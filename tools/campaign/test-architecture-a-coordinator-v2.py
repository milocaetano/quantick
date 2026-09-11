"""Offline transport tests. Import under a subprocess/network tripwire."""
import copy
import datetime
import importlib.util
import json
import pathlib
import tempfile
import unittest
from unittest.mock import patch

HERE = pathlib.Path(__file__).parent
with patch('subprocess.run', side_effect=AssertionError('Network/process call during import')), patch('pathlib.Path.mkdir', side_effect=AssertionError('Import mkdir')):
    spec = importlib.util.spec_from_file_location('coordinator_v2', HERE / 'architecture-a-coordinator-v2.py')
    c = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(c)

TIME = datetime.datetime(2026, 9, 9, 5, tzinfo=datetime.timezone.utc)
WRITER = 'offline/test'


def state(sequence=1, previous=None):
    return {'schema': 1, 'campaign': 'milocaetano/quantick#330', 'sequence': sequence,
            'publication_key': f'campaign/checkpoint-{sequence}/{WRITER}', 'previous': previous,
            'created_at': TIME.isoformat(), 'writer': WRITER, 'phase': 'running',
            'base_sha': 'a' * 40, 'integration': {'current_sha': 'b' * 40},
            'authorization': {'source': 'https://example.test/actual-retained-grant', 'allowed': ['issues'], 'excluded': ['main_merge']},
            'lease': {'owner': WRITER, 'expires_at': (TIME + datetime.timedelta(minutes=15)).isoformat()},
            'project': {'id': 'P1', 'pending': []}, 'tasks': [{'key': 'Q8', 'state': 'backlog', 'attempts': {'repair': 2, 'operation': 0}, 'failures': [{'signature': 'F1', 'attempts': 2}]}],
            'inflight': None, 'decisions': [], 'metrics': {'score': 'unchanged'},
            'retry_policy': {'operation_attempts': 3, 'repair_attempts': 3},
            'next_action': {'task': 'Q8'}, 'stop_reason': None, 'operation_history': []}


class FakeTransport:
    def __init__(self):
        self.rows = []
        self.writes = 0
        self.lose_after_append = False
        self.fail_before_append = False
        self.on_append = None

    def seed(self, text):
        url = f'https://github.com/milocaetano/quantick/issues/330#issuecomment-{len(self.rows) + 1}'
        self.rows.append({'url': url, 'body': text})
        return url

    def append(self, text):
        self.writes += 1
        if self.fail_before_append:
            self.fail_before_append = False
            raise TimeoutError('unknown transport result')
        url = self.seed(text)
        if self.on_append:
            self.on_append(self)
        if self.lose_after_append:
            self.lose_after_append = False
            raise TimeoutError('write happened, response lost')
        return url

    def read(self, url):
        for row in self.rows:
            if row['url'] == url:
                return copy.deepcopy(row)
        raise c.ReconcileError('Missing part/comment')

    def comments_since(self, url):
        for index, row in enumerate(self.rows):
            if row['url'] == url:
                return copy.deepcopy(self.rows[index:])
        raise c.ReconcileError('Expected checkpoint absent')


class CoordinatorTests(unittest.TestCase):
    def setUp(self):
        self.t = FakeTransport()
        self.base = state()
        self.url = self.t.seed(c.body(c.CHECKPOINT, self.base))
        self.p = c.Publisher(self.t, WRITER, lambda: TIME)

    def proposed(self):
        result = copy.deepcopy(self.base)
        result.update(sequence=2, previous=self.url, publication_key='campaign/checkpoint-2/' + WRITER)
        return result

    def operation(self, status='pending'):
        return {'campaign_id': self.base['campaign'], 'checkpoint': self.url,
                'operation_key': 'project-field-Q8', 'attempt': 1, 'owner': WRITER,
                'lease_expires_at': self.base['lease']['expires_at'], 'target': 'Project item1',
                'intent': {'field': 'state', 'value': 'Ready'}, 'expected_readback': {'id': 'item1', 'value': 'Ready'},
                'status': status, 'evidence': [] if status == 'pending' else ['https://example.test/readback'],
                'authorization': self.base['authorization']}

    def large(self):
        result = self.proposed()
        result['tasks'][0]['evidence_blob'] = 'unicode é漢字 ' * 4000
        return result

    def test_inactive_import_and_gh(self):
        self.assertIsNone(c._publisher)
        with self.assertRaises(c.InactiveError):
            c.gh('issue', 'comment', '330')

    def test_legacy_checkpoint_exact_read(self):
        self.assertEqual(c.read_checkpoint(self.url, self.t), self.base)
        oversized = self.base | {'old_evidence': 'x' * 60000}
        url = self.t.seed(c.body(c.CHECKPOINT, oversized))
        self.assertEqual(c.read_checkpoint(url, self.t), oversized)

    def test_small_checkpoint_preserves_unknown_fields(self):
        s = self.proposed() | {'future_optional': {'nested': ['x', 7]}}
        url = self.p.checkpoint(s, self.url)
        self.assertEqual(c.read_checkpoint(url, self.t), s)
        self.assertEqual(self.t.writes, 1)

    def test_partition_utf8_integrity_and_bounds(self):
        s = self.large()
        url = self.p.checkpoint(s, self.url)
        self.assertEqual(c.read_checkpoint(url, self.t), s)
        for row in self.t.rows[1:]:
            self.assertLessEqual(len(row['body'].encode()), 24 * 1024)
        manifest = c.parse(self.t.read(url)['body'], c.CHECKPOINT)
        self.assertGreater(manifest['part_count'], 1)
        self.assertEqual(manifest['task_count'], 1)
        self.assertTrue(all(row['body'].startswith(c.PART) for row in self.t.rows[1:-1]))

    def test_fifty_active_entries_force_partition(self):
        s = self.proposed()
        s['tasks'] = [{'key': str(i), 'state': 'ready'} for i in range(50)]
        self.assertIsNone(c.encode_checkpoint(s)['complete'])

    def test_missing_part_refuses_complete_state(self):
        url = self.p.checkpoint(self.large(), self.url)
        del self.t.rows[1]
        with self.assertRaises(c.ReconcileError):
            c.read_checkpoint(url, self.t)

    def test_corrupt_part_digest_refused(self):
        url = self.p.checkpoint(self.large(), self.url)
        part = c.parse(self.t.rows[1]['body'], c.PART)
        part['sha256'] = '0' * 64
        self.t.rows[1]['body'] = c.body(c.PART, part)
        with self.assertRaises(c.ReconcileError):
            c.read_checkpoint(url, self.t)

    def test_manifest_wrong_task_count_refused(self):
        url = self.p.checkpoint(self.large(), self.url)
        manifest = c.parse(self.t.rows[-1]['body'], c.CHECKPOINT)
        manifest['task_count'] += 1
        self.t.rows[-1]['body'] = c.body(c.CHECKPOINT, manifest)
        with self.assertRaises(c.ReconcileError):
            c.read_checkpoint(url, self.t)

    def test_duplicate_part_url_refused(self):
        url = self.p.checkpoint(self.large(), self.url)
        manifest = c.parse(self.t.rows[-1]['body'], c.CHECKPOINT)
        manifest['parts'][1]['url'] = manifest['parts'][0]['url']
        self.t.rows[-1]['body'] = c.body(c.CHECKPOINT, manifest)
        with self.assertRaises(c.ReconcileError):
            c.read_checkpoint(url, self.t)

    def test_partial_publication_is_uncommitted_and_resumable(self):
        s = self.large()
        parts = c.encode_checkpoint(s)['parts']
        self.t.seed(parts[0])
        self.assertEqual(c.read_checkpoint(self.url, self.t), self.base)
        with self.assertRaises(c.ReconcileError):
            self.p.boundary(self.url, s['campaign'])
        url = self.p.checkpoint(s, self.url)
        self.assertEqual(c.read_checkpoint(url, self.t), s)
        self.assertEqual(sum(row['body'] == parts[0] for row in self.t.rows), 1)

    def test_lost_checkpoint_response_reread_no_duplicate(self):
        self.t.lose_after_append = True
        s = self.proposed()
        url = self.p.checkpoint(s, self.url)
        self.assertEqual(self.p.checkpoint(s, self.url), url)
        self.assertEqual(self.t.writes, 1)

    def test_lost_part_response_reread_no_duplicate(self):
        self.t.lose_after_append = True
        s = self.large()
        url = self.p.checkpoint(s, self.url)
        expected = len(c.encode_checkpoint(s)['parts']) + 1
        self.assertEqual(self.t.writes, expected)
        self.assertEqual(c.read_checkpoint(url, self.t), s)

    def test_unobserved_append_stops_without_automatic_retry(self):
        self.t.fail_before_append = True
        with self.assertRaises(c.UncertainAppend):
            self.p.checkpoint(self.proposed(), self.url)
        self.assertEqual(self.t.writes, 1)
        self.assertEqual(len(self.t.rows), 1)

    def test_operation_pending_duplicate_idempotent(self):
        record = self.operation()
        url = self.p.journal(record)
        self.assertEqual(self.p.journal(record), url)
        self.assertEqual(self.t.writes, 1)
        self.assertLessEqual(len(self.t.rows[-1]['body'].encode()), 8192)

    def test_operation_conflicting_payload_blocks(self):
        self.p.journal(self.operation())
        bad = self.operation() | {'target': 'other item'}
        with self.assertRaises(c.ReconcileError):
            self.p.journal(bad)
        self.assertEqual(self.t.writes, 1)

    def test_operation_result_requires_pending(self):
        with self.assertRaises(c.ReconcileError):
            self.p.journal(self.operation('succeeded'))
        self.assertEqual(self.t.writes, 0)

    def test_pending_result_and_checkpoint_fold(self):
        pending = self.p.journal(self.operation())
        result = self.p.journal(self.operation('succeeded'))
        s = self.proposed()
        with self.assertRaises(c.ReconcileError):
            self.p.checkpoint(s, self.url)
        s['consumed_journal_records'] = [pending, result]
        s['operation_history'] = [{'key': 'project-field-Q8', 'attempts': 1,
                                  'last_status': 'succeeded',
                                  'result_evidence': ['https://example.test/readback'],
                                  'journal_urls': [pending, result]}]
        url = self.p.checkpoint(s, self.url)
        self.assertEqual(c.read_checkpoint(url, self.t), s)
        self.assertEqual(self.t.writes, 3)

    def test_unresolved_operation_blocks_new_work_and_snapshot(self):
        self.p.journal(self.operation())
        with self.assertRaises(c.ReconcileError):
            self.p.journal(self.operation() | {'operation_key': 'second'})
        with self.assertRaises(c.ReconcileError):
            self.p.checkpoint(self.proposed(), self.url)

    def test_oversized_journal_no_write(self):
        with self.assertRaises(c.ReconcileError):
            self.p.journal(self.operation() | {'intent': 'x' * 9000})
        self.assertEqual(self.t.writes, 0)

    def test_failed_attempt_retry_keeps_counts(self):
        self.p.journal(self.operation())
        self.p.journal(self.operation('failed'))
        self.p.journal(self.operation() | {'attempt': 2})
        with self.assertRaises(c.ReconcileError):
            self.p.journal(self.operation() | {'attempt': 4})

    def test_success_is_not_retried_as_new_attempt(self):
        self.p.journal(self.operation())
        self.p.journal(self.operation('succeeded'))
        with self.assertRaises(c.ReconcileError):
            self.p.journal(self.operation() | {'attempt': 2})

    def test_counter_removal_and_decrease_block(self):
        for transform in ('remove', 'decrease'):
            s = self.proposed()
            if transform == 'remove':
                s['tasks'][0]['failures'] = []
            else:
                s['tasks'][0]['attempts']['repair'] = 1
            with self.assertRaises(c.ReconcileError):
                self.p.checkpoint(s, self.url)

    def test_foreign_writer_and_new_boundary_stop_writes(self):
        foreign = self.operation() | {'owner': 'other'}
        self.t.seed(c.body(c.OPERATION, foreign))
        with self.assertRaises(c.ReconcileError):
            self.p.boundary(self.url, self.base['campaign'])
        self.assertEqual(self.t.writes, 0)

    def test_new_boundary_stops_old_coordinator(self):
        self.t.seed(c.body(c.CHECKPOINT, self.proposed() | {'writer': 'other', 'publication_key': 'other'}))
        with self.assertRaises(c.ReconcileError):
            self.p.checkpoint(self.proposed(), self.url)

    def test_sibling_before_cached_boundary_is_not_timestamp_winner(self):
        sibling = self.proposed() | {'publication_key': 'sibling'}
        self.t.seed(c.body(c.CHECKPOINT, sibling))
        latest = self.t.seed(c.body(c.CHECKPOINT, self.proposed()))
        with self.assertRaises(c.ReconcileError):
            self.p.boundary(latest, self.base['campaign'])

    def test_expired_lease_stops_writes(self):
        self.p.clock = lambda: TIME + datetime.timedelta(hours=1)
        with self.assertRaises(c.ReconcileError):
            self.p.journal(self.operation())

    def test_duplicate_json_key_rejected(self):
        with self.assertRaises(c.ReconcileError):
            c.parse(c.CHECKPOINT + '\n{"schema":1,"schema":1}', c.CHECKPOINT)

    def test_wrapper_pending_checkpoint_persists_failed_response(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
            local = copy.deepcopy(self.base)
            local['previous'] = self.url
            original = copy.deepcopy(local)
            self.t.fail_before_append = True
            with self.assertRaises(c.UncertainAppend):
                c.checkpoint(local)
            self.assertEqual(local, original)
            saved = json.loads((pathlib.Path(tmp) / 'v2-pending-checkpoint.json').read_text())
            url = c.checkpoint(local)
            self.assertEqual(c.read_checkpoint(url, self.t), saved['proposed'])
            self.assertEqual(local['tasks'][0]['attempts']['repair'], 2)
            self.assertFalse((pathlib.Path(tmp) / 'v2-pending-checkpoint.json').exists())

    def test_operation_lost_response_readback_is_idempotent(self):
        self.t.lose_after_append = True
        record = self.operation()
        pending = self.p.journal(record)
        self.assertEqual(self.p.journal(record), pending)
        self.t.lose_after_append = True
        done = self.p.journal(self.operation('succeeded'))
        self.assertEqual(self.p.journal(self.operation('succeeded')), done)
        self.assertEqual(self.t.writes, 2)

    def test_result_without_evidence_refused(self):
        self.p.journal(self.operation())
        with self.assertRaises(c.ReconcileError):
            self.p.journal(self.operation('succeeded') | {'evidence': []})

    def test_fork_arriving_during_append_invalidates_publication(self):
        def competing(t):
            t.on_append = None
            t.seed(c.body(c.CHECKPOINT, self.proposed() | {'publication_key': 'competing', 'writer': 'other'}))
        self.t.on_append = competing
        with self.assertRaises(c.ReconcileError):
            self.p.checkpoint(self.proposed(), self.url)
        self.assertEqual(self.t.writes, 1)

    def test_whole_snapshot_digest_mismatch_refused(self):
        url = self.p.checkpoint(self.large(), self.url)
        manifest = c.parse(self.t.rows[-1]['body'], c.CHECKPOINT)
        manifest['snapshot_bytes'] += 1
        self.t.rows[-1]['body'] = c.body(c.CHECKPOINT, manifest)
        with self.assertRaises(c.ReconcileError):
            c.read_checkpoint(url, self.t)

    def test_duplicate_task_identity_refused(self):
        s = self.proposed()
        s['tasks'].append(copy.deepcopy(s['tasks'][0]))
        with self.assertRaises(c.ReconcileError):
            c.encode_checkpoint(s)

    def test_unsupported_manifest_format_refused(self):
        url = self.p.checkpoint(self.large(), self.url)
        manifest = c.parse(self.t.rows[-1]['body'], c.CHECKPOINT)
        manifest['format'] = 'unrecognized:v9'
        self.t.rows[-1]['body'] = c.body(c.CHECKPOINT, manifest)
        with self.assertRaises(c.ReconcileError):
            c.read_checkpoint(url, self.t)

    def test_success_and_unknown_history_after_checkpoint_refuse_retry(self):
        for status in ('succeeded', None):
            historical = state()
            historical['operation_history'] = [{'key': 'project-field-Q8', 'attempts': 1}]
            if status:
                historical['operation_history'][0]['last_status'] = status
            self.t.rows[0]['body'] = c.body(c.CHECKPOINT, historical)
            with self.assertRaises(c.ReconcileError):
                self.p.journal(self.operation() | {'attempt': 2})
        self.assertEqual(self.t.writes, 0)

    def test_failed_history_after_checkpoint_preserves_next_attempt(self):
        historical = state()
        historical['operation_history'] = [{'key': 'project-field-Q8', 'attempts': 1,
                                            'last_status': 'failed', 'result_evidence': ['https://example.test/failure']}]
        self.t.rows[0]['body'] = c.body(c.CHECKPOINT, historical)
        self.p.journal(self.operation() | {'attempt': 2})
        self.assertEqual(self.t.writes, 1)

    def test_wrapper_folds_terminal_operation_evidence(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
            self.p.journal(self.operation())
            self.p.journal(self.operation('succeeded'))
            local = copy.deepcopy(self.base)
            local['previous'] = self.url
            url = c.checkpoint(local)
            row = c.read_checkpoint(url, self.t)['operation_history'][0]
            self.assertEqual(row['attempts'], 1)
            self.assertEqual(row['last_status'], 'succeeded')
            self.assertEqual(row['result_evidence'], ['https://example.test/readback'])
            self.assertEqual(len(row['journal_urls']), 2)


class ReservedRepairTests(unittest.TestCase):
    setUp = CoordinatorTests.setUp
    proposed = CoordinatorTests.proposed
    operation = CoordinatorTests.operation

    def seed_history(self, history):
        self.base['operation_history'] = copy.deepcopy(history)
        self.t.rows[0]['body'] = c.body(c.CHECKPOINT, self.base)

    def local(self):
        return copy.deepcopy(self.base) | {'previous': self.url}

    def test_ir01_unknown_history_deletion_refused(self):
        self.seed_history([{'key': 'old-operation', 'attempts': None,
                            'last_status': None, 'history': 'unknown'}])
        with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
            local = self.local()
            local['operation_history'] = []
            with self.assertRaises(c.ReconcileError):
                c.checkpoint(local)
            self.assertEqual(self.t.writes, 0)

    def test_ir01_unknown_history_cannot_be_numbered(self):
        self.seed_history([{'key': 'old-operation', 'attempts': None,
                            'last_status': None, 'history': 'unknown'}])
        proposed = self.proposed()
        proposed['operation_history'][0].update(attempts=1, last_status='failed')
        with self.assertRaises(c.ReconcileError):
            self.p.checkpoint(proposed, self.url)
        self.assertEqual(self.t.writes, 0)

    def test_ir01_unknown_attempt_refuses_even_with_failed_status(self):
        self.seed_history([{'key': 'project-field-Q8', 'attempts': None,
                            'last_status': 'failed', 'history': 'unknown'}])
        with self.assertRaises(c.ReconcileError):
            self.p.journal(self.operation())
        with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
            with self.assertRaises(c.ReconcileError):
                c.intent(self.local(), 'project-field-Q8', 'item', {}, {})
        self.assertEqual(self.t.writes, 0)

    def test_ir01_unchanged_unknown_survives_checkpoint(self):
        original = [{'key': 'old-operation', 'attempts': None,
                     'last_status': None, 'history': 'unknown', 'source': 'historical-record'}]
        self.seed_history(original)
        with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
            local = self.local()
            url = c.checkpoint(local)
            self.assertEqual(c.read_checkpoint(url, self.t)['operation_history'], original)
            with self.assertRaises(c.ReconcileError):
                c.intent(local, 'old-operation', 'item', {}, {})
            self.assertEqual(self.t.writes, 1)

    def test_ir02_observed_success_cannot_be_downgraded(self):
        pending = self.p.journal(self.operation())
        result = self.p.journal(self.operation('succeeded'))
        proposed = self.proposed()
        proposed['consumed_journal_records'] = [pending, result]
        proposed['operation_history'] = [{'key': 'project-field-Q8', 'attempts': 1,
                                         'last_status': 'failed', 'result_evidence': [],
                                         'journal_urls': [pending, result]}]
        with self.assertRaises(c.ReconcileError):
            self.p.checkpoint(proposed, self.url)
        self.assertEqual(self.t.writes, 2)

    def test_ir02_historical_status_evidence_and_urls_preserved(self):
        original = [{'key': 'project-field-Q8', 'attempts': 1, 'last_status': 'succeeded',
                     'result_evidence': ['https://example.test/success'],
                     'journal_urls': ['https://example.test/historical-journal']}]
        for field, replacement in [('last_status', 'failed'), ('result_evidence', []), ('journal_urls', [])]:
            with self.subTest(field=field):
                self.setUp()
                self.seed_history(original)
                with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
                    local = self.local()
                    local['operation_history'][0][field] = replacement
                    with self.assertRaises(c.ReconcileError):
                        c.checkpoint(local)
                    self.assertEqual(self.t.writes, 0)

    def test_ir02_failed_retry_folds_actual_outcome_and_retains_prior_urls(self):
        old_url = 'https://example.test/earlier-failed-journal'
        self.seed_history([{'key': 'project-field-Q8', 'attempts': 1, 'last_status': 'failed',
                            'result_evidence': ['https://example.test/failure'], 'journal_urls': [old_url]}])
        pending = self.p.journal(self.operation() | {'attempt': 2})
        result = self.p.journal(self.operation('succeeded') | {'attempt': 2})
        with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
            local = self.local()
            url = c.checkpoint(local)
            row = c.read_checkpoint(url, self.t)['operation_history'][0]
            self.assertEqual(row['attempts'], 2)
            self.assertEqual(row['last_status'], 'succeeded')
            self.assertEqual(row['result_evidence'], ['https://example.test/readback'])
            self.assertEqual(row['journal_urls'], [old_url, pending, result])
            with self.assertRaises(c.ReconcileError):
                c.intent(local, 'project-field-Q8', 'item', {}, {})

    def test_ir03_frozen_publication_blocks_intent_then_exact_resume_allows_it(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
            local = self.local()
            self.t.fail_before_append = True
            with self.assertRaises(c.UncertainAppend):
                c.checkpoint(local)
            path = pathlib.Path(tmp) / 'v2-pending-checkpoint.json'
            frozen = path.read_bytes()
            with self.assertRaises(c.ReconcileError):
                c.intent(local, 'new-unrelated-operation', 'item', {}, {})
            self.assertEqual(self.t.writes, 1)
            self.assertEqual(path.read_bytes(), frozen)
            url = c.checkpoint(local)
            self.assertEqual(c.read_checkpoint(url, self.t), json.loads(frozen)['proposed'])
            self.assertFalse(path.exists())
            pending = c.intent(local, 'new-unrelated-operation', 'item', {}, {})
            self.assertEqual(pending['record']['attempt'], 1)
            self.assertEqual(self.t.writes, 3)

    def test_ir03_frozen_publication_blocks_direct_journal(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
            self.t.fail_before_append = True
            with self.assertRaises(c.UncertainAppend):
                c.checkpoint(self.local())
            with self.assertRaises(c.ReconcileError):
                self.p.journal(self.operation())
            self.assertEqual(self.t.writes, 1)


class ReservedSecondRepairTests(unittest.TestCase):
    setUp = CoordinatorTests.setUp
    proposed = CoordinatorTests.proposed

    def local(self):
        return copy.deepcopy(self.base) | {'previous': self.url}

    def test_ir04_premature_checkpoint_leaves_result_and_later_checkpoint_available(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
            local = self.local()
            original = copy.deepcopy(local)
            pending = c.intent(local, 'observed-operation', 'item', {'change': 'ready'}, {'state': 'ready'})
            with self.assertRaisesRegex(c.ReconcileError, 'Unresolved operation'):
                c.checkpoint(local)
            self.assertFalse((pathlib.Path(tmp) / 'v2-pending-checkpoint.json').exists())
            self.assertEqual(local, original)
            self.assertEqual(self.t.writes, 1)
            result = c.result(pending, 'succeeded', ['https://example.test/observed-ready'])
            url = c.checkpoint(local)
            published = c.read_checkpoint(url, self.t)
            self.assertEqual(published['operation_history'], [{'key': 'observed-operation',
                'attempts': 1, 'last_status': 'succeeded',
                'result_evidence': ['https://example.test/observed-ready'],
                'journal_urls': [pending['url'], result]}])
            self.assertEqual(published['consumed_journal_records'], [pending['url'], result])
            self.assertEqual(self.t.writes, 3)
            self.assertFalse((pathlib.Path(tmp) / 'v2-pending-checkpoint.json').exists())

    def test_ir04_known_invalid_proposals_do_not_freeze(self):
        for invalid in ('decreased_counter', 'duplicate_task', 'missing_phase', 'base_inflight'):
            with self.subTest(invalid=invalid):
                self.setUp()
                with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
                    local = self.local()
                    if invalid == 'decreased_counter':
                        local['tasks'][0]['attempts']['repair'] = 1
                    elif invalid == 'duplicate_task':
                        local['tasks'].append(copy.deepcopy(local['tasks'][0]))
                    elif invalid == 'missing_phase':
                        del local['phase']
                    else:
                        self.base['inflight'] = {'key': 'legacy-unresolved-operation'}
                        self.t.rows[0]['body'] = c.body(c.CHECKPOINT, self.base)
                        local = self.local()
                    original = copy.deepcopy(local)
                    with self.assertRaises(c.ReconcileError):
                        c.checkpoint(local)
                    self.assertFalse((pathlib.Path(tmp) / 'v2-pending-checkpoint.json').exists())
                    self.assertEqual(local, original)
                    self.assertEqual(self.t.writes, 0)

    def test_ir04_actual_partial_publication_retains_frozen_cache_until_exact_resume(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
            local = self.local()
            local['tasks'][0]['evidence_blob'] = 'retained evidence ' * 4000
            def fail_after_first_part(transport):
                transport.on_append = None
                transport.fail_before_append = True
            self.t.on_append = fail_after_first_part
            with self.assertRaises(c.UncertainAppend):
                c.checkpoint(local)
            path = pathlib.Path(tmp) / 'v2-pending-checkpoint.json'
            frozen = path.read_bytes()
            first_part = self.t.rows[1]['body']
            self.assertTrue(first_part.startswith(c.PART))
            self.assertEqual(len(self.t.rows), 2)
            with self.assertRaises(c.ReconcileError):
                c.intent(local, 'new-operation', 'item', {}, {})
            self.assertEqual(path.read_bytes(), frozen)
            url = c.checkpoint(local)
            self.assertEqual(c.read_checkpoint(url, self.t), json.loads(frozen)['proposed'])
            self.assertEqual(sum(row['body'] == first_part for row in self.t.rows), 1)
            self.assertFalse(path.exists())


    def test_ir05_actual_review_repair_batches_cannot_decrease_or_disappear(self):
        for value in ('delete', 0, None):
            with self.subTest(value=value):
                self.setUp()
                self.base['tasks'][0].update(key='Q10', review_repair_batches=3)
                self.t.rows[0]['body'] = c.body(c.CHECKPOINT, self.base)
                proposed = self.proposed()
                if value == 'delete':
                    del proposed['tasks'][0]['review_repair_batches']
                else:
                    proposed['tasks'][0]['review_repair_batches'] = value
                with self.assertRaises(c.ReconcileError):
                    self.p.checkpoint(proposed, self.url)
                self.assertEqual(self.t.writes, 0)

    def test_ir05_unknown_review_repair_batches_cannot_be_numbered_or_deleted(self):
        for value in ('delete', 0, 1):
            with self.subTest(value=value):
                self.setUp()
                self.base['tasks'][0].update(key='Q10', review_repair_batches=None)
                self.t.rows[0]['body'] = c.body(c.CHECKPOINT, self.base)
                with tempfile.TemporaryDirectory() as tmp, patch.object(c, 'ROOT', pathlib.Path(tmp)), patch.object(c, '_publisher', self.p):
                    local = self.local()
                    if value == 'delete':
                        del local['tasks'][0]['review_repair_batches']
                    else:
                        local['tasks'][0]['review_repair_batches'] = value
                    with self.assertRaises(c.ReconcileError):
                        c.checkpoint(local)
                    self.assertEqual(self.t.writes, 0)
                    self.assertFalse((pathlib.Path(tmp) / 'v2-pending-checkpoint.json').exists())

    def test_ir05_unknown_review_repair_batches_is_retained(self):
        self.base['tasks'][0].update(key='Q10', review_repair_batches=None)
        self.t.rows[0]['body'] = c.body(c.CHECKPOINT, self.base)
        proposed = self.proposed()
        url = self.p.checkpoint(proposed, self.url)
        self.assertIsNone(c.read_checkpoint(url, self.t)['tasks'][0]['review_repair_batches'])
        self.assertEqual(self.t.writes, 1)

    def test_ir05_known_review_batch_increase_allows_stalled_batch_reset(self):
        self.base['tasks'][0].update(key='Q10', review_repair_batches=3, stalled_batches=2)
        self.t.rows[0]['body'] = c.body(c.CHECKPOINT, self.base)
        proposed = self.proposed()
        proposed['tasks'][0].update(review_repair_batches=4, stalled_batches=0)
        url = self.p.checkpoint(proposed, self.url)
        self.assertEqual(c.read_checkpoint(url, self.t)['tasks'][0]['review_repair_batches'], 4)
        self.assertEqual(c.read_checkpoint(url, self.t)['tasks'][0]['stalled_batches'], 0)
        self.assertEqual(self.t.writes, 1)


if __name__ == '__main__':
    unittest.main(verbosity=2)
