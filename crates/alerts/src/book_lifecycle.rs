//! Mutation and durable restoration are cold paths.
use super::*;
use quantick_civil::{CivilDate, DAY_MS, TzOffset};
impl AlertBook {
    pub fn command(
        &mut self,
        command: AlertCommand,
        utc_ms: i64,
    ) -> Result<CommandResult, AlertError> {
        self.utc_ms = utc_ms;
        match command {
            AlertCommand::ClearHistory => {
                self.log.clear();
                Ok(CommandResult {
                    id: None,
                    revision: 0,
                })
            }
            AlertCommand::Create(mut spec) => {
                self.validate(&spec)?;
                if self.entries.len() == MAX_ALERTS {
                    return Err(AlertError::Limit {
                        resource: "alerts",
                        maximum: MAX_ALERTS,
                    });
                }
                if spec.id.0 != 0 {
                    return Err(editor::invalid("id", "new definitions require id zero"));
                }
                let next = self
                    .next_id
                    .checked_add(1)
                    .ok_or_else(|| editor::invalid("id", "exhausted"))?;
                spec.id = AlertId(self.next_id);
                self.next_id = next;
                let id = spec.id;
                self.entries.push(Entry::new(SavedAlert {
                    spec,
                    revision: 1,
                    enabled: true,
                    completed: false,
                    last_fired_utc_ms: None,
                    last_fired_session_ms: None,
                    last_bar_epoch: None,
                    last_bar_sequence: None,
                }));
                Ok(CommandResult {
                    id: Some(id),
                    revision: 1,
                })
            }
            command => {
                let (id, expected) = match &command {
                    AlertCommand::Edit {
                        expected_revision,
                        spec,
                    } => (spec.id, *expected_revision),
                    AlertCommand::Pause {
                        id,
                        expected_revision,
                    }
                    | AlertCommand::Resume {
                        id,
                        expected_revision,
                    }
                    | AlertCommand::Rearm {
                        id,
                        expected_revision,
                    }
                    | AlertCommand::Delete {
                        id,
                        expected_revision,
                    } => (*id, *expected_revision),
                    _ => unreachable!(),
                };
                if let AlertCommand::Edit { spec, .. } = &command {
                    self.validate(spec)?;
                }
                let index = self
                    .entries
                    .iter()
                    .position(|entry| entry.saved.spec.id == id)
                    .ok_or(AlertError::UnknownId)?;
                let entry = &mut self.entries[index];
                if entry.saved.revision != expected {
                    return Err(AlertError::StaleRevision {
                        actual: entry.saved.revision,
                    });
                }
                let revision = expected
                    .checked_add(1)
                    .ok_or_else(|| editor::invalid("revision", "exhausted"))?;
                match command {
                    AlertCommand::Delete { .. } => {
                        self.entries.remove(index);
                    }
                    other => {
                        match other {
                            AlertCommand::Edit { spec, .. } => entry.saved.spec = spec,
                            AlertCommand::Pause { .. } => entry.saved.enabled = false,
                            AlertCommand::Resume { .. } => entry.saved.enabled = true,
                            AlertCommand::Rearm { .. } => {
                                entry.saved.completed = false;
                                entry.saved.enabled = true;
                                entry.saved.last_bar_epoch = None;
                                entry.saved.last_bar_sequence = None;
                                entry.saved.last_fired_session_ms = None;
                                entry.saved.last_fired_utc_ms = None;
                            }
                            _ => unreachable!(),
                        }
                        entry.saved.revision = revision;
                        entry.clear();
                        entry.status = AlertStatus::Suspended(Unavailable::SourceInactive);
                    }
                }
                Ok(CommandResult {
                    id: Some(id),
                    revision,
                })
            }
        }
    }
    pub fn restore(&mut self, document: AlertDocument, utc_ms: i64) -> Result<(), AlertError> {
        if document.version != FORMAT_VERSION {
            return Err(AlertError::UnsupportedVersion(document.version));
        }
        if document.definitions.len() > MAX_ALERTS || document.history.len() > MAX_HISTORY {
            return Err(editor::invalid("document", "exceeds bounded capacity"));
        }
        let mut restored = Self {
            utc_ms,
            ..Self::default()
        };
        for saved in document.definitions {
            self.validate(&saved.spec)?;
            if saved.spec.id.0 == 0
                || saved.spec.id.0 >= document.next_id
                || saved.revision == 0
                || restored
                    .entries
                    .iter()
                    .any(|entry| entry.saved.spec.id == saved.spec.id)
            {
                return Err(editor::invalid(
                    "definitions",
                    "invalid or duplicate identity/revision",
                ));
            }
            let mut entry = Entry::new(saved);
            entry.clear();
            restored.entries.push(entry);
        }
        let mut sequence = 0;
        for event in document.history {
            self.validate(&event.spec)?;
            if event.sequence <= sequence
                || event.sequence >= document.next_event_sequence
                || !event.price.is_finite()
                || event.definition_revision == 0
            {
                return Err(editor::invalid("history", "invalid sequence or event"));
            }
            sequence = event.sequence;
            restored.log.push_back(event);
        }
        if document.next_id == 0 || document.next_event_sequence == 0 {
            return Err(editor::invalid("document", "invalid counters"));
        }
        restored.next_id = document.next_id;
        restored.next_event_sequence = document.next_event_sequence;
        *self = restored;
        Ok(())
    }
    pub fn resolve_expiration(
        &self,
        choice: ExpirationChoice,
        utc_ms: i64,
        display_offset_minutes: i32,
    ) -> Result<Expiration, AlertError> {
        editor::offset(display_offset_minutes)?;
        let tz = TzOffset::new(display_offset_minutes);
        let date = CivilDate::from_ms(utc_ms, tz);
        let cutoff = match choice {
            ExpirationChoice::Never => return Ok(Expiration::Never),
            ExpirationChoice::Custom { utc_ms } => utc_ms,
            ExpirationChoice::EndOfDay => date.end_ms(tz),
            ExpirationChoice::Week => utc_ms
                .checked_add(7 * DAY_MS)
                .ok_or_else(|| editor::invalid("expiration", "overflow"))?,
            ExpirationChoice::CalendarMonth => {
                let next = date.offset_months(1);
                let days = next.offset_months(1).day_number() - next.day_number();
                let day = date.ymd().2.min(days) - 1;
                let time = utc_ms
                    .checked_sub(date.start_ms(tz))
                    .ok_or_else(|| editor::invalid("expiration", "overflow"))?;
                next.start_ms(tz)
                    .checked_add(day * DAY_MS)
                    .and_then(|v| v.checked_add(time))
                    .ok_or_else(|| editor::invalid("expiration", "overflow"))?
            }
        };
        if cutoff <= utc_ms {
            return Err(editor::invalid("expiration", "must be in the future"));
        }
        Ok(Expiration::At {
            utc_ms: cutoff,
            display_offset_minutes,
        })
    }
}
