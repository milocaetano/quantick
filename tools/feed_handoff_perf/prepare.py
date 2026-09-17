"""Prepare immutable-source benchmark exports; never run a timing sample."""
from pathlib import Path
import argparse,datetime,difflib,hashlib,json,subprocess,sys
from source import extract_archive

tool_root=Path(__file__).resolve().parent
p=argparse.ArgumentParser()
p.add_argument('--candidate-commit',required=True)
p.add_argument('--repo',required=True,type=Path)
p.add_argument('--output',required=True,type=Path)
p.add_argument('--control',default='f217fcf3db65dac0fc1cbb98956d9155000522ac')
a=p.parse_args()
repo=candidate=a.repo.resolve()
root=a.output.resolve()
base=a.control
assert base=='f217fcf3db65dac0fc1cbb98956d9155000522ac', 'F2 control is fixed by protocol reconciliation'
assert not root.is_relative_to(repo), 'Exports must be outside the clean checkout'
root.mkdir(parents=True,exist_ok=False)
receipt={'command':[sys.executable,*sys.argv], 'candidate':a.candidate_commit,'control':base,
         'start_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'status':'started','exit':None}
receipt_path=root/'prepare-receipt.json'
receipt_path.write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
def failed(kind,error,traceback):
    receipt.update(status='failed',exit=1,error=str(error),finish_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
    receipt_path.write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
    sys.__excepthook__(kind,error,traceback)
sys.excepthook=failed
def git(*args):return subprocess.check_output(['git','-C',str(repo),*args])
def sha(data):return hashlib.sha256(data).hexdigest()
observed=subprocess.check_output(['git','-C',str(candidate),'rev-parse','HEAD']).decode().strip()
receipt['observed_candidate']=observed
receipt_path.write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
assert observed==a.candidate_commit, 'Candidate must be exact current committed HEAD'
assert not subprocess.check_output(['git','-C',str(candidate),'status','--porcelain']), 'Candidate tracked/untracked tree must be clean'
assert len(a.candidate_commit)==40 and len(base)==40, 'Immutable full commits required'
common=(tool_root/'common.rs').read_bytes()
fixture=(tool_root/'fixture.json').read_bytes()
assert sha(fixture)=='b7e798913b7ddd7269949c60c4035da39f3d212b0656ebf0bcd76878dd268d65'
manifest={'base':base,'candidate_commit':a.candidate_commit,'common_sha256':sha(common),'fixture_sha256':sha(fixture),'sides':{}}
for side in ['control','candidate']:
    commit=base if side=='control' else a.candidate_commit
    archive=git('archive','--format=zip',commit)
    (root/(side+'-original-source.zip')).write_bytes(archive)
    dest=root/side
    if dest.exists():raise SystemExit(f'Refuse to overwrite existing source export: {dest}')
    extract_archive(archive,dest)
    originals={name:(dest/name).read_text(encoding='utf-8') for name in ['crates/feed/src/lib.rs','crates/feed/src/binance.rs','crates/feed/src/hyperliquid.rs']}
    # Common harness invokes production host functions. Adapters only supply
    # local endpoints and unused book channels, without editing timed logic.
    binance=dest/'crates/feed/src/binance.rs'
    binance.write_text(binance.read_text(encoding='utf-8')+'''
#[cfg(test)]
pub(crate) async fn f2_bench_host(rest: String, url: String, tx: mpsc::Sender<FeedEvent>, notice_tx: mpsc::Sender<FeedNotice>, cmd_rx: mpsc::Receiver<FeedCommand>) {
    let (book_tx, _book_rx) = mpsc::channel(8192);
    feed_task("BTCUSDT".into(), tx, book_tx, notice_tx, cmd_rx, BinanceSource {
        http: BinanceHttp::with_base_url(rest), url,
        backoff: Backoff::new(std::time::Duration::from_secs(1),std::time::Duration::from_secs(1),7),
    }).await;
}
''',encoding='utf-8')
    hl=dest/'crates/feed/src/hyperliquid.rs'
    text=hl.read_text(encoding='utf-8')
    if side=='control':
        # Exact private endpoint injection only; source mapper, legacy watch,
        # channel capacities and the entire host loop remain unmodified.
        old='runtime.block_on(feed_task(symbol, tx, book_tx, notice_tx, cmd_rx));'
        assert old in text
        text=text.replace(old,'runtime.block_on(feed_task(symbol, tx, book_tx, notice_tx, cmd_rx, HYPERLIQUID_WS_URL.to_owned()));')
        old='    mut cmd_rx: mpsc::Receiver<FeedCommand>,\n) {'
        assert old in text
        text=text.replace(old,'    mut cmd_rx: mpsc::Receiver<FeedCommand>,\n    fixture_url: String,\n) {',1)
        old='            HYPERLIQUID_WS_URL,\n'
        assert old in text
        text=text.replace(old,'            &fixture_url,\n',1)
        arg='url'
        event_type='FeedEvent'
        function='feed_task'
        output='tx'
    else:
        arg='HyperliquidSource { local_fixture: false, url, backoff: Backoff::for_feed(TRADE_RECONNECT_SEED) }'
        event_type='crate::ObservedFeedEvent'
        function='feed_task_with'
        output='ObservedOutput(tx)'
    text+='''
#[cfg(test)]
pub(crate) async fn f2_bench_host(url: String, tx: mpsc::Sender<'''+event_type+'''>, notice_tx: mpsc::Sender<FeedNotice>, cmd_rx: mpsc::Receiver<FeedCommand>) {
    let (book_tx, _book_rx) = mpsc::channel(8192);
    '''+function+'''("BTC".into(), '''+output+''', book_tx, notice_tx, cmd_rx, '''+arg+''').await;
}
'''
    hl.write_text(text,encoding='utf-8')
    lib=dest/'crates/feed/src/lib.rs'
    if side=='candidate':
        receiver_adapter='''
#[cfg(test)] type F2BenchEvent = ObservedFeedEvent;
#[cfg(test)] type F2BenchReceiver = ObservedReceiver;
#[cfg(test)] fn f2_bench_receiver<R: Into<ObservedReceiver>>(receiver:R) -> ObservedReceiver { receiver.into() }
#[cfg(test)] fn f2_bench_observation(event:ObservedFeedEvent) -> f2_protocol::Observation {
    match event {
        ObservedFeedEvent::Feed(event) => f2_protocol::Observation::Feed(event),
        ObservedFeedEvent::Excluded(event) => match event.reason {
            ExclusionReason::MalformedRow => f2_protocol::Observation::Excluded { malformed:event.rows.get(), stale:0 },
            ExclusionReason::StaleTimestamp => f2_protocol::Observation::Excluded { malformed:0, stale:event.rows.get() },
        },
    }
}
'''
    else:
        receiver_adapter='''
#[cfg(test)] type F2BenchEvent = FeedEvent;
#[cfg(test)] type F2BenchReceiver = mpsc::Receiver<FeedEvent>;
#[cfg(test)] fn f2_bench_receiver(receiver:mpsc::Receiver<FeedEvent>) -> F2BenchReceiver { receiver }
#[cfg(test)] fn f2_bench_observation(event:FeedEvent) -> f2_protocol::Observation { f2_protocol::Observation::Feed(event) }
'''
    lib.write_text(lib.read_text(encoding='utf-8')+'\n#[cfg(test)]\nmod f2_protocol;\n#[cfg(test)]\nfn f2_bench_candidate() -> bool { '+str(side=='candidate').lower()+' }\n'+receiver_adapter,encoding='utf-8')
    (dest/'crates/feed/src/f2_protocol.rs').write_bytes(common)
    (dest/'crates/feed/src/f2-fixture.json').write_bytes(fixture)
    patch=''.join(''.join(difflib.unified_diff(original.splitlines(True),(dest/name).read_text(encoding='utf-8').splitlines(True),fromfile=name,tofile=name)) for name,original in originals.items())
    (root/(side+'-instrumentation.patch')).write_text(patch,encoding='utf-8')
    manifest['sides'][side]={'commit':commit,'tree':git('rev-parse',commit+'^{tree}').decode().strip(),'source_archive_sha256':sha(archive),'instrumentation_patch_sha256':sha(patch.encode()),'instrumented':{str(p.relative_to(dest)):sha(p.read_bytes()) for p in [lib,binance,hl]}}
(root/'source-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
receipt.update(status='passed',exit=0,finish_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
receipt_path.write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
print('Prepared source exports only; no benchmark executed')
