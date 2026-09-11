"""Inactive external campaign journal/checkpoint helper; see its companion README.

Import performs no I/O. No legacy coordinator is imported or modified.
The explicit versioned partition format is an external implementation choice.
"""
from __future__ import annotations

import base64
import copy
import datetime
import hashlib
import json
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).parent / 'quantick-architecture-a'
REPO = 'milocaetano/quantick'
WRITER = 'codex/01a07841-8868-75a1-b6f6-9cb93837d72d'
CHECKPOINT = '<!-- quantick-campaign-checkpoint:v1 -->'
PART = '<!-- quantick-campaign-checkpoint-part:v1 -->'
OPERATION = '<!-- campaign-operation:v1 -->'
FORMAT = 'quantick-snapshot-parts:v1'
SNAPSHOT_LIMIT = 24 * 1024
JOURNAL_LIMIT = 8 * 1024
CHUNK_BYTES = 12 * 1024
# Existing operational policy: renewal duration, partition trigger and read page.
DEFAULT_LEASE_MINUTES = 15
ACTIVE_TASK_PARTITION_THRESHOLD = 50
COMMENT_PAGE_SIZE = 5
_publisher = None


class ReconcileError(RuntimeError):
    pass


class InactiveError(ReconcileError):
    pass


class UncertainAppend(ReconcileError):
    pass


def now():
    return datetime.datetime.now(datetime.timezone.utc)


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(',', ':'), allow_nan=False).encode('utf-8')


def digest(value):
    return hashlib.sha256(value).hexdigest()


def body(marker, value):
    return marker + '\n```json\n' + canonical(value).decode('utf-8') + '\n```\n'


def parse(text, marker):
    if not text.startswith(marker):
        raise ReconcileError('Wrong record marker')
    tail = text[len(marker):].strip()
    if tail.startswith('```json\n') and tail.endswith('\n```'):
        tail = tail[len('```json\n'):-len('\n```')]
    try:
        def unique(pairs):
            result = {}
            for key, value in pairs:
                if key in result:
                    raise ValueError('Duplicate JSON key')
                result[key] = value
            return result
        value = json.loads(tail, object_pairs_hook=unique,
                           parse_constant=lambda _: (_ for _ in ()).throw(ValueError()))
    except (ValueError, TypeError) as exc:
        raise ReconcileError('Malformed record JSON') from exc
    if not isinstance(value, dict):
        raise ReconcileError('Record must be an object')
    return value


def bounded(text, ceiling):
    if len(text.encode('utf-8')) > ceiling:
        raise ReconcileError('Record exceeds byte bound; link payload evidence before retry')
    return text


def validate_state(state):
    required = {'schema', 'campaign', 'sequence', 'publication_key', 'previous',
                'created_at', 'writer', 'phase', 'base_sha', 'integration',
                'authorization', 'lease', 'project', 'tasks', 'inflight', 'decisions',
                'metrics', 'retry_policy', 'next_action', 'stop_reason'}
    if required - state.keys() or state['schema'] != 1:
        raise ReconcileError('Missing state fields or unsupported checkpoint schema')
    if type(state['sequence']) is not int or state['sequence'] < 1:
        raise ReconcileError('Invalid sequence')
    if not isinstance(state['tasks'], list):
        raise ReconcileError('Invalid tasks')
    keys = [task.get('key') for task in state['tasks'] if isinstance(task, dict)]
    if len(keys) != len(state['tasks']) or None in keys or len(set(keys)) != len(keys):
        raise ReconcileError('Task identities must exist and be unique')
    canonical(state)


def encode_checkpoint(state):
    """Return a complete body, or immutable part bodies plus manifest metadata."""
    validate_state(state)
    raw = canonical(state)
    text = body(CHECKPOINT, state)
    active = sum(t.get('state') != 'done' for t in state['tasks'])
    if len(raw) <= SNAPSHOT_LIMIT and len(text.encode('utf-8')) <= SNAPSHOT_LIMIT and active < ACTIVE_TASK_PARTITION_THRESHOLD:
        return {'complete': text, 'parts': []}
    chunks = [raw[i:i + CHUNK_BYTES] for i in range(0, len(raw), CHUNK_BYTES)]
    parts = []
    for index, chunk in enumerate(chunks):
        part = {'schema': 1, 'format': FORMAT, 'campaign': state['campaign'],
                'sequence': state['sequence'], 'publication_key': state['publication_key'],
                'writer': state['writer'], 'index': index, 'count': len(chunks),
                'snapshot_sha256': digest(raw), 'bytes': len(chunk),
                'sha256': digest(chunk), 'encoding': 'base64',
                'payload': base64.b64encode(chunk).decode('ascii')}
        parts.append(bounded(body(PART, part), SNAPSHOT_LIMIT))
    manifest = {key: copy.deepcopy(state[key]) for key in (
        'schema', 'campaign', 'sequence', 'publication_key', 'previous', 'created_at',
        'writer', 'phase', 'lease', 'inflight')}
    manifest.update({'kind': 'partition_manifest', 'format': FORMAT,
                     'snapshot_sha256': digest(raw), 'snapshot_bytes': len(raw),
                     'part_count': len(parts), 'task_count': len(state['tasks']),
                     'active_task_count': active, 'parts': []})
    return {'complete': None, 'parts': parts, 'manifest': manifest}


def read_checkpoint(url, transport):
    """Strict reader. A manifest is never returned as if it were full state."""
    obj = parse(transport.read(url)['body'], CHECKPOINT)
    if obj.get('kind') != 'partition_manifest':
        validate_state(obj)
        return obj
    if obj.get('format') != FORMAT or obj.get('schema') != 1:
        raise ReconcileError('Unsupported partition format')
    bounded(body(CHECKPOINT, obj), SNAPSHOT_LIMIT)
    entries = obj.get('parts', [])
    if not entries or len(entries) != obj.get('part_count'):
        raise ReconcileError('Missing part entries')
    if len({entry.get('url') for entry in entries}) != len(entries):
        raise ReconcileError('Duplicate part URL')
    chunks = []
    for index, entry in enumerate(entries):
        if entry.get('index') != index:
            raise ReconcileError('Part ordering mismatch')
        part_text = transport.read(entry['url'])['body']
        bounded(part_text, SNAPSHOT_LIMIT)
        part = parse(part_text, PART)
        for key in ('schema', 'format', 'campaign', 'sequence', 'publication_key',
                    'writer', 'snapshot_sha256'):
            if part.get(key) != obj.get(key):
                raise ReconcileError('Cross-snapshot part identity mismatch: ' + key)
        if part.get('index') != index or part.get('count') != len(entries) or part.get('encoding') != 'base64':
            raise ReconcileError('Part index/count/encoding mismatch')
        try:
            chunk = base64.b64decode(part['payload'], validate=True)
        except (ValueError, KeyError) as exc:
            raise ReconcileError('Invalid part payload') from exc
        if (part.get('sha256') != digest(chunk) or entry.get('sha256') != digest(chunk)
                or part.get('bytes') != len(chunk) or entry.get('bytes') != len(chunk)):
            raise ReconcileError('Part digest/length mismatch')
        chunks.append(chunk)
    raw = b''.join(chunks)
    if len(raw) != obj.get('snapshot_bytes') or digest(raw) != obj.get('snapshot_sha256'):
        raise ReconcileError('Snapshot digest/length mismatch')
    state = parse(CHECKPOINT + '\n' + raw.decode('utf-8'), CHECKPOINT)
    validate_state(state)
    if canonical(state) != raw:
        raise ReconcileError('Reassembled bytes are not canonical JSON')
    for key in ('schema', 'campaign', 'sequence', 'publication_key', 'previous',
                'created_at', 'writer', 'phase', 'lease', 'inflight'):
        if state.get(key) != obj.get(key):
            raise ReconcileError('Manifest/full-state identity mismatch: ' + key)
    if len(state['tasks']) != obj.get('task_count') or sum(t.get('state') != 'done' for t in state['tasks']) != obj.get('active_task_count'):
        raise ReconcileError('Task count mismatch')
    return state


def counter_map(value, path=(), counted=False):
    result = {}
    if isinstance(value, dict):
        for key, item in value.items():
            track = counted or key in {'attempts', 'batches', 'observed_failures', 'review_repair_batches'}
            if key == 'review_repair_batches' and item is None:
                result[path + (key,)] = None
            else:
                result.update(counter_map(item, path + (key,), track))
    elif isinstance(value, list):
        for index, item in enumerate(value):
            identity = next((str(item[k]) for k in ('key', 'signature', 'id') if isinstance(item, dict) and k in item), str(index))
            result.update(counter_map(item, path + (identity,), counted))
    elif counted and type(value) is int:
        result[path] = value
    return result


def preserve_counters(old, new):
    old_counts, new_counts = counter_map(old), counter_map(new)
    for path, value in old_counts.items():
        if path not in new_counts:
            raise ReconcileError('Counter removed: ' + '/'.join(path))
        current = new_counts[path]
        if ((value is None and current is not None)
                or (value is not None and (current is None or current < value))):
            raise ReconcileError('Counter decreased or unknown history changed: ' + '/'.join(path))


def _history_index(state):
    rows = state.get('operation_history', [])
    if not isinstance(rows, list):
        raise ReconcileError('Operation history must be a list')
    indexed = {}
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get('key'), str) or not row['key'] or row['key'] in indexed:
            raise ReconcileError('Operation history identities must exist and be unique')
        indexed[row['key']] = row
    return indexed


def _history_attempts(state, key):
    row = _history_index(state).get(key)
    if row is None:
        return []
    count = row.get('attempts')
    if type(count) is not int or count < 0:
        raise ReconcileError('Historical attempt count unknown; evidenced recovery required before retry')
    return [count]


def _project_history(base, operations):
    """Preserve all prior rows; only observed journal results advance history."""
    history = copy.deepcopy(_history_index(base))
    for (key, attempt), observed in sorted(operations.items(), key=lambda entry: entry[0][1]):
        prior = history.get(key)
        if prior is not None:
            count = _history_attempts({'operation_history': [prior]}, key)[0]
            if prior.get('last_status') != 'failed':
                raise ReconcileError('Historical success or unknown outcome cannot be retried')
        else:
            count = 0
        if attempt != count + 1:
            raise ReconcileError('Observed journal does not continue preserved operation count')
        row = copy.deepcopy(prior) if prior is not None else {'key': key}
        urls = row.get('journal_urls', [])
        if not isinstance(urls, list):
            raise ReconcileError('Historical journal provenance requires explicit recovery')
        row.update(attempts=attempt, last_status=observed['record']['status'],
                   result_evidence=copy.deepcopy(observed['record']['evidence']),
                   journal_urls=urls + [url for url in observed['urls'] if url not in urls])
        history[key] = row
    return list(history.values())


def _refuse_frozen_checkpoint():
    if (ROOT / 'v2-pending-checkpoint.json').exists():
        raise ReconcileError('Frozen checkpoint publication unresolved; reconcile its exact result before operation work')


def operation_identity(record):
    return {key: value for key, value in record.items() if key not in {'status', 'evidence'}}


def fold_operations(rows, campaign, checkpoint_url):
    """Validate pending/result order; return records with all provenance URLs."""
    grouped = {}
    for row in rows:
        if not row['body'].startswith(OPERATION):
            continue
        bounded(row['body'], JOURNAL_LIMIT)
        record = parse(row['body'], OPERATION)
        required = {'campaign_id', 'checkpoint', 'operation_key', 'attempt', 'owner',
                    'lease_expires_at', 'target', 'intent', 'expected_readback', 'status',
                    'evidence', 'authorization'}
        if required - record.keys() or record['campaign_id'] != campaign or record['checkpoint'] != checkpoint_url:
            raise ReconcileError('Journal scope/required-field mismatch')
        if type(record['attempt']) is not int or record['attempt'] < 1 or record['status'] not in {'pending', 'succeeded', 'failed'}:
            raise ReconcileError('Invalid operation attempt/status')
        if not isinstance(record['evidence'], list) or (record['status'] != 'pending' and not record['evidence']):
            raise ReconcileError('Result requires observed evidence references')
        key = (record['operation_key'], record['attempt'])
        prior = grouped.get(key)
        if prior:
            if operation_identity(prior['record']) != operation_identity(record):
                raise ReconcileError('Conflicting operation identity/payload')
            if prior['record']['status'] == record['status']:
                if canonical(prior['record']) != canonical(record):
                    raise ReconcileError('Conflicting duplicate operation')
                prior['urls'].append(row['url'])
                continue
            if prior['record']['status'] != 'pending' or record['status'] == 'pending':
                raise ReconcileError('Conflicting operation result/order')
        elif record['status'] != 'pending':
            raise ReconcileError('Result without pending intent')
        urls = prior['urls'] + [row['url']] if prior else [row['url']]
        grouped[key] = {'record': record, 'urls': urls}
    return grouped


class Publisher:
    """Transport and clock injected; no network or local state writes in init."""
    def __init__(self, transport, writer, clock=now):
        self.transport, self.writer, self.clock = transport, writer, clock

    def boundary(self, url, campaign, allow_publication=None):
        base = read_checkpoint(url, self.transport)
        if base['campaign'] != campaign:
            raise ReconcileError('Wrong campaign boundary')
        # Inspect the preceding complete boundary too: a sibling publication can
        # precede the caller's cached URL and must not win merely by timestamp.
        rows = self.transport.comments_since(base.get('previous') or url)
        if not any(row['url'] == url for row in rows):
            raise ReconcileError('Expected boundary missing')
        boundary_index = next(i for i, row in enumerate(rows) if row['url'] == url)
        for row in rows[:boundary_index]:
            if row['body'].startswith(CHECKPOINT):
                sibling = parse(row['body'], CHECKPOINT)
                if sibling.get('sequence') == base['sequence'] and row['body'] != self.transport.read(url)['body']:
                    raise ReconcileError('Conflicting sibling checkpoint before cached boundary')
        later = rows[boundary_index + 1:]
        for row in later:
            if row['body'].startswith(CHECKPOINT):
                candidate = parse(row['body'], CHECKPOINT)
                if candidate.get('publication_key') != allow_publication:
                    raise ReconcileError('New or conflicting checkpoint; reconcile ownership')
            elif row['body'].startswith(PART):
                candidate = parse(row['body'], PART)
                if candidate.get('writer') != self.writer or candidate.get('publication_key') != allow_publication:
                    raise ReconcileError('Uncommitted checkpoint publication requires reconciliation')
        operations = fold_operations(later, campaign, url)
        if any(item['record']['owner'] != self.writer for item in operations.values()):
            raise ReconcileError('Different operation writer after expected boundary')
        lease = base.get('lease')
        if not lease or lease.get('owner') != self.writer:
            raise ReconcileError('Acquire/recover ownership through a reviewed checkpoint first')
        expiry = datetime.datetime.fromisoformat(lease['expires_at'].replace('Z', '+00:00'))
        # Renewals are represented only by explicit root ownership checkpoints here.
        if expiry <= self.clock():
            raise ReconcileError('Lease expired; explicit recovery required')
        return base, later, operations

    def append_once(self, text, marker, identity, boundary, campaign, publication=None):
        """Read before append and after a lost response; never blindly retry."""
        def observed():
            _, rows, _ = self.boundary(boundary, campaign, publication)
            found = []
            for row in rows:
                if row['body'].startswith(marker):
                    obj = parse(row['body'], marker)
                    if all(obj.get(key) == value for key, value in identity.items()):
                        if row['body'] != text:
                            raise ReconcileError('Same publication identity has different bytes')
                        found.append(row['url'])
            return found
        found = observed()
        if found:
            return found[0]
        try:
            url = self.transport.append(text)
        except Exception as exc:
            found = observed()
            if found:
                return found[0]
            raise UncertainAppend('Append response lost and no exact record found; retain frozen payload, reconcile before retry') from exc
        if self.transport.read(url)['body'] != text:
            raise ReconcileError('Append readback differs from frozen body')
        found = observed()
        if url not in found:
            raise ReconcileError('Appended record missing from subsequent boundary readback')
        return url

    def journal(self, record):
        _refuse_frozen_checkpoint()
        record = copy.deepcopy(record)
        text = bounded(body(OPERATION, record), JOURNAL_LIMIT)
        base, _, grouped = self.boundary(record['checkpoint'], record['campaign_id'])
        if record.get('owner') != self.writer or record.get('authorization') != base['authorization']:
            raise ReconcileError('Journal owner/authority projection mismatch')
        if record.get('lease_expires_at') != base['lease']['expires_at']:
            raise ReconcileError('Journal lease differs from current boundary')
        key = (record['operation_key'], record['attempt'])
        prior = grouped.get(key)
        historical = _history_attempts(base, record['operation_key'])
        # Reconstruct pending+result for validation rather than inventing a new attempt.
        _, rows, _ = self.boundary(record['checkpoint'], record['campaign_id'])
        fold_operations(rows + [{'url': 'proposed', 'body': text}], record['campaign_id'], record['checkpoint'])
        if record['status'] == 'pending' and not prior:
            if any(x['record']['operation_key'] == record['operation_key'] and x['record']['status'] == 'succeeded' for x in grouped.values()):
                raise ReconcileError('Operation already succeeded; reuse readback rather than retry')
            historical_rows = [x for x in base.get('operation_history', []) if x.get('key') == record['operation_key']]
            if historical_rows and not any(name == record['operation_key'] for name, _ in grouped):
                latest = max(historical_rows, key=lambda x: x['attempts'])
                if latest.get('last_status') != 'failed':
                    raise ReconcileError('Historical operation succeeded or outcome unknown; recover actual readback before retry')
            if base.get('inflight') or any(x['record']['status'] == 'pending' for x in grouped.values()):
                raise ReconcileError('Another operation remains unresolved')
            previous_attempts = [attempt for (name, attempt) in grouped if name == record['operation_key']]
            maximum = max(previous_attempts + historical, default=0)
            if record['attempt'] != maximum + 1:
                raise ReconcileError('Operation attempt is not the next preserved count')
            if record['attempt'] > base['retry_policy']['operation_attempts']:
                raise ReconcileError('Operation attempt limit exhausted')
        return self.append_once(text, OPERATION, {'operation_key': key[0], 'attempt': key[1],
                                'status': record['status']}, record['checkpoint'], record['campaign_id'])

    def _prepare_checkpoint(self, state, expected_url):
        """Read-only validation shared by pre-freeze and publication checks."""
        base, _, operations = self.boundary(expected_url, state['campaign'], state['publication_key'])
        if state['previous'] != expected_url or state['sequence'] != base['sequence'] + 1 or state['writer'] != self.writer:
            raise ReconcileError('Checkpoint lineage/writer mismatch')
        preserve_counters(base, state)
        if base.get('inflight') or any(x['record']['status'] == 'pending' for x in operations.values()):
            raise ReconcileError('Unresolved operation blocks complete checkpoint')
        expected_journals = [url for item in operations.values() for url in item['urls']]
        if not set(expected_journals) <= set(state.get('consumed_journal_records', [])):
            raise ReconcileError('Checkpoint omits consumed journal provenance')
        expected_history = _project_history(base, operations)
        if _history_index(state) != _history_index({'operation_history': expected_history}):
            raise ReconcileError('Checkpoint operation history contradicts preserved history or observed journal')
        encoded = encode_checkpoint(state)
        if not encoded['complete']:
            # Reject already-oversized metadata before freezing or appending parts.
            # The final manifest is checked again when actual part URLs exist.
            bounded(body(CHECKPOINT, encoded['manifest']), SNAPSHOT_LIMIT)
        return encoded

    def checkpoint(self, state, expected_url):
        state = copy.deepcopy(state)
        encoded = self._prepare_checkpoint(state, expected_url)
        if encoded['complete']:
            text = encoded['complete']
        else:
            manifest = encoded['manifest']
            for index, part_text in enumerate(encoded['parts']):
                part = parse(part_text, PART)
                url = self.append_once(part_text, PART, {'publication_key': state['publication_key'], 'index': index},
                                       expected_url, state['campaign'], state['publication_key'])
                manifest['parts'].append({'index': index, 'url': url, 'bytes': part['bytes'], 'sha256': part['sha256']})
            text = bounded(body(CHECKPOINT, manifest), SNAPSHOT_LIMIT)
        url = self.append_once(text, CHECKPOINT, {'publication_key': state['publication_key']}, expected_url,
                               state['campaign'], state['publication_key'])
        if read_checkpoint(url, self.transport) != state:
            raise ReconcileError('Final committed snapshot differs from input')
        return url


class GhTransport:
    def __init__(self, parent_number):
        self.parent_number = parent_number

    def read(self, url):
        match = re.fullmatch(r'https://github\.com/milocaetano/quantick/issues/' + str(self.parent_number) + r'#issuecomment-(\d+)', url)
        if not match:
            raise ReconcileError('Comment URL is outside the expected parent')
        obj = data('api', f'repos/{REPO}/issues/comments/{match[1]}')
        if obj.get('issue_url') != f'https://api.github.com/repos/{REPO}/issues/{self.parent_number}':
            raise ReconcileError('Comment belongs to another issue')
        return {'url': obj['html_url'], 'body': obj['body']}

    def comments_since(self, boundary):
        query = ('query($number:Int!,$before:String){repository(owner:"milocaetano",name:"quantick"){issue(number:$number){comments(last:'
                 + str(COMMENT_PAGE_SIZE)
                 + ',before:$before){nodes{url body}pageInfo{hasPreviousPage startCursor}}}}}')
        cursor, rows = None, []
        while True:
            args = ['api', 'graphql', '-f', 'query=' + query, '-F', 'number=' + str(self.parent_number)]
            if cursor:
                args += ['-f', 'before=' + cursor]
            page = data(*args)['data']['repository']['issue']['comments']
            rows = page['nodes'] + rows
            if any(x['url'] == boundary for x in page['nodes']):
                return rows
            if not page['pageInfo']['hasPreviousPage']:
                raise ReconcileError('Expected boundary absent from all comment pages')
            cursor = page['pageInfo']['startCursor']

    def append(self, text):
        return gh('issue', 'comment', str(self.parent_number), '--repo', REPO,
                  '--body-file', file('v2-frozen-publication.md', text))


def activate(receipt, *, transport=None):
    """Root calls only after actual reviewed Q10 merge/adoption; no authority granted."""
    global _publisher
    required = ('adoption_checkpoint', 'reviewed_sync_merge', 'source_main_sha', 'format')
    if any(not receipt.get(key) for key in required) or receipt['format'] != FORMAT:
        raise InactiveError('Recorded reviewed synchronization/adoption receipt required')
    for key in ('reviewed_sync_merge', 'source_main_sha'):
        if not re.fullmatch('[0-9a-f]{40}', receipt[key]):
            raise InactiveError('Full source and merge SHAs required')
    if transport is None:
        transport = GhTransport(receipt.get('parent_number', 330))
    _publisher = Publisher(transport, WRITER)
    return _publisher


def gh(*args, cwd=None):
    if _publisher is None:
        raise InactiveError('External v2 helper has not been activated after reviewed adoption')
    proc = subprocess.run(['gh', *args], cwd=cwd, capture_output=True, text=True, encoding='utf-8')
    if proc.returncode:
        raise RuntimeError(proc.stderr.strip())
    return proc.stdout.strip()


def data(*args):
    return json.loads(gh(*args))


def file(name, value):
    path = ROOT / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value, encoding='utf-8', newline='\n')
    return str(path)


def _active():
    if _publisher is None:
        raise InactiveError('Record reviewed Q10 adoption and activate explicitly first')
    return _publisher


def intent(s, key, target, mutation, expected, *, attempt=None):
    publisher = _active()
    _refuse_frozen_checkpoint()
    base, _, records = publisher.boundary(s['previous'], s['campaign'])
    historical = _history_attempts(base, key)
    existing = [x['record'] for (name, _), x in records.items() if name == key]
    pending = next((x for x in existing if x['status'] == 'pending'), None)
    if attempt is None:
        if pending:
            attempt = pending['attempt']
        else:
            prior = [x['attempt'] for x in existing] + historical
            attempt = max(prior, default=0) + 1
    record = {'campaign_id': s['campaign'], 'checkpoint': s['previous'], 'operation_key': key,
              'attempt': attempt, 'owner': publisher.writer, 'lease_expires_at': base['lease']['expires_at'],
              'target': target, 'intent': mutation, 'expected_readback': expected, 'status': 'pending',
              'evidence': [], 'authorization': base['authorization']}
    url = publisher.journal(record)
    # Journal is authoritative; no task counters or full checkpoint changed here.
    return {'url': url, 'record': record}


def result(pending, status, evidence):
    if status not in {'succeeded', 'failed'} or not evidence:
        raise ReconcileError('Observed result and evidence required')
    record = copy.deepcopy(pending['record'])
    record.update(status=status, evidence=evidence)
    return _active().journal(record)


def checkpoint(s, operation=None, *, release=False):
    """Explicit recovery boundary only; old implicit intent/result behavior is refused."""
    publisher = _active()
    if operation is not None:
        raise ReconcileError('Use intent/result journal; checkpoint(operation) is not silently migrated')
    request = {'state': s, 'release': release}
    path = ROOT / 'v2-pending-checkpoint.json'
    if path.exists():
        saved = json.loads(path.read_text(encoding='utf-8'))
        if saved['request_sha256'] != digest(canonical(request)):
            raise ReconcileError('Frozen prior checkpoint still pending; reconcile it before changing state')
        proposed = saved['proposed']
    else:
        proposed = copy.deepcopy(s)
        base, _, operations = publisher.boundary(s['previous'], s['campaign'])
        proposed['sequence'] = base['sequence'] + 1
        proposed['writer'] = publisher.writer
        proposed['created_at'] = publisher.clock().isoformat()
        proposed['publication_key'] = f'{s["campaign"]}/checkpoint-{proposed["sequence"]}/{publisher.writer}'
        proposed['lease'] = None if release else {'owner': publisher.writer, 'expires_at': (publisher.clock() + datetime.timedelta(minutes=DEFAULT_LEASE_MINUTES)).isoformat()}
        proposed['consumed_journal_records'] = [url for item in operations.values() for url in item['urls']]
        history = _project_history(base, operations)
        supplied = _history_index(proposed)
        if supplied != _history_index(base) and supplied != _history_index({'operation_history': history}):
            raise ReconcileError('Caller operation history contradicts prior boundary or observed journal')
        proposed['operation_history'] = history
        publisher._prepare_checkpoint(proposed, s['previous'])
        saved = {'request_sha256': digest(canonical(request)), 'proposed': proposed}
        file(path.name, json.dumps(saved, ensure_ascii=False, indent=2))
    url = publisher.checkpoint(proposed, s['previous'])
    committed = copy.deepcopy(proposed)
    committed['previous'] = url  # Existing caller convention: pointer to current public boundary.
    file('state.json', json.dumps(committed, ensure_ascii=False, indent=2))
    s.clear()
    s.update(committed)
    path.unlink()
    return url
