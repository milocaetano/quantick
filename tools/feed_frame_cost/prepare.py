"""Prepare external exports from frozen original archives; no product/build execution."""
from pathlib import Path
import argparse, difflib, hashlib, json
from lifecycle import Stage, write
from source import extract_archive
from verify import ARCHIVES, TOOLS, archive_files, verify_exports
HERE=Path(__file__).resolve().parent
CONTROL=ARCHIVES['control'][0]
CANDIDATE=ARCHIVES['candidate'][0]
sha=lambda data:hashlib.sha256(data).hexdigest()
base_adapter='''type ProtocolEvent = EVENT;
fn wrap(event: FeedEvent) -> ProtocolEvent { WRAP }
fn attach(app: &mut QuantickApp, receiver: mpsc::Receiver<ProtocolEvent>, book_events: mpsc::Receiver<DepthEvent>, commands: mpsc::Sender<FeedCommand>) {
    app.active_tab_mut().attach_for_test(HANDLE {
        events: RECEIVER, book_events, notices: feed::silent_notices(),
        capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
        latency: feed::unsplit_latency(), commands, replay: None,
    });
}
'''
control=base_adapter.replace('EVENT','FeedEvent').replace('WRAP','event').replace('HANDLE','FeedHandle').replace('RECEIVER','receiver')+'''
fn send_exclusions(_: &mpsc::Sender<ProtocolEvent>) {}
fn assert_delivery(_: &QuantickApp) {}
fn assert_paint(_: &QuantickApp, texts: &[String]) { assert!(!texts.is_empty()); }
fn delivery_text(_: &QuantickApp) -> Vec<String> { Vec::new() }
fn prime_panel(_: &QuantickApp, _: &egui::Context, _: egui::Vec2) { panic!("control has no delivery panel"); }
fn panel_frame(_: &QuantickApp, _: &egui::Context, _: egui::Vec2, _: bool) -> (egui::FullOutput,u128) { panic!("control has no delivery panel"); }
'''
candidate=base_adapter.replace('EVENT','quantick_feed::ObservedFeedEvent').replace('WRAP','quantick_feed::ObservedFeedEvent::Feed(event)').replace('HANDLE','quantick_feed::ObservedFeedHandle').replace('RECEIVER','receiver.into()')+'''
fn send_exclusions(events: &mpsc::Sender<ProtocolEvent>) {
    for reason in [quantick_feed::ExclusionReason::MalformedRow,quantick_feed::ExclusionReason::StaleTimestamp] {
        events.try_send(quantick_feed::ObservedFeedEvent::Excluded(quantick_feed::FeedExclusion { reason,rows:std::num::NonZeroU64::new(1).unwrap() })).unwrap();
    }
}
fn assert_delivery(app: &QuantickApp) {
    let view=&app.active_tab().feed_delivery;
    assert!(view.reporting);
    assert_eq!((view.exclusions.malformed_rows,view.exclusions.stale_rows),(1,1));
    assert!(!view.text().0[0].contains("LOCAL SYNTHETIC"));
}
fn delivery_text(app: &QuantickApp) -> Vec<String> { app.active_tab().feed_delivery.text().0.to_vec() }
fn assert_paint(app: &QuantickApp, texts: &[String]) {
    for text in delivery_text(app) { assert!(texts.contains(&text),"actual panel missing {text:?}"); }
}
fn prime_panel(app: &QuantickApp, ctx: &egui::Context, size: egui::Vec2) {
    let _=ctx.run(egui::RawInput { screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,size)),..Default::default() },|ctx|crate::feed_integrity_view::draw_panel(ctx,&app.active_tab().feed_delivery.text()));
}
fn panel_frame(app: &QuantickApp, ctx: &egui::Context, size: egui::Vec2, reference: bool) -> (egui::FullOutput,u128) {
    let mut elapsed=0;
    let output=ctx.run(egui::RawInput { screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,size)),..Default::default() },|ctx| {
        elapsed=crate::feed_integrity_view::f2_panel_measure(ctx,&app.active_tab().feed_delivery,reference);
    });
    (output,elapsed)
}
'''
panel='''
#[cfg(test)]
pub(crate) fn f2_panel_measure(ctx: &egui::Context, view: &DeliveryView, reference: bool) -> u128 {
    let retained=reference.then(||ctx.data(|data|data.get_temp::<CachedGalleys>(egui::Id::new("feed.delivery.readout"))).expect("cache primed before sample"));
    let started=std::time::Instant::now();
    if let Some(cache)=retained { paint_panel(ctx,cache); } else { draw_panel(ctx,&view.text()); }
    started.elapsed().as_nanos()
}
'''

control += "\nfn panel_geometry(_: &QuantickApp, _: &egui::Context, _: &egui::FullOutput) -> serde_json::Value { serde_json::Value::Null }\n"
control += "\nfn panel_proof(_: &QuantickApp, _: &egui::Context, _: egui::Vec2, _: bool) -> egui::FullOutput { panic!(\"control has no delivery panel\"); }\n"

candidate += "\nfn panel_geometry(app: &QuantickApp, ctx: &egui::Context, output: &egui::FullOutput) -> serde_json::Value { crate::feed_integrity_view::f2_panel_geometry(ctx,output,&app.active_tab().feed_delivery) }\n"
candidate += '''
fn panel_proof(app: &QuantickApp, ctx: &egui::Context, size: egui::Vec2, reference: bool) -> egui::FullOutput {
    ctx.run(egui::RawInput { screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,size)),..Default::default() }, |ctx| {
        crate::feed_integrity_view::f2_panel_proof(ctx,&app.active_tab().feed_delivery,reference);
    })
}
'''

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archives',required=True,type=Path)
    parser.add_argument('--output',required=True,type=Path)
    args=parser.parse_args()
    root=args.output.resolve()
    if root.exists(): raise ValueError('Refuse existing preparation output')
    with Stage(root,'prepare') as stage:
        common=(HERE/'common.rs').read_bytes()
        manifest={'control':CONTROL,'candidate':CANDIDATE,'common_sha256':sha(common),'sides':{},'protocol':{'sizes':{'normal':[1440,900],'narrow':[1000,700]},'scale':1.5,'seed':8000,'warmup_frames':30,'measured_frames':600,'prints_per_frame':64,'warmup_pairs':3,'measured_pairs':15,'gates':{'median_ratio':1.05,'p95_ratio':1.10,'cv':.05},'diagnostics':{'unknown':3,'missing':0,'non_monotonic':0,'malformed':1,'stale':1,'local_synthetic':False}}}
        manifest['tools']={name:sha((HERE/name).read_bytes()) for name in TOOLS}
        for side, adapter in [('control',control),('candidate',candidate)]:
            commit,tree,expected=ARCHIVES[side]
            archive=(args.archives/(side+'-original-source.zip')).read_bytes()
            if sha(archive)!=expected: raise ValueError('Frozen archive hash mismatch')
            archive_files(archive)
            (root/(side+'-original-source.zip')).write_bytes(archive)
            dest=root/side
            extract_archive(archive,dest)
            mods='crates/app/src/app/tests/mod.rs'
            panelpath='crates/app/src/feed_integrity_view.rs'
            originals={mods:(dest/mods).read_bytes()}
            (dest/mods).write_bytes(originals[mods]+b'\nmod f2_frame_protocol_tests;\n')
            commonpath='crates/app/src/app/tests/f2_frame_protocol_tests.rs'
            adapterpath='crates/app/src/app/tests/f2_frame_adapter.rs'
            (dest/commonpath).write_bytes(common)
            (dest/adapterpath).write_bytes(adapter.encode())
            if side=='candidate':
                originals[panelpath]=(dest/panelpath).read_bytes()
                (dest/panelpath).write_bytes(originals[panelpath]+panel.encode()+(HERE/'geometry.rs').read_bytes())
            originals[commonpath]=b''
            originals[adapterpath]=b''
            patch=''.join(''.join(difflib.unified_diff(old.decode().splitlines(True),(dest/name).read_text(encoding='utf-8').splitlines(True),fromfile=name,tofile=name)) for name,old in originals.items())
            (root/(side+'-instrumentation.patch')).write_bytes(patch.encode())
            manifest['sides'][side]={'commit':commit,'tree':tree,'archive_sha256':sha(archive),'patch_sha256':sha(patch.encode()),'instrumented':{name:sha((dest/name).read_bytes()) for name in originals},'lock_sha256':sha((dest/'Cargo.lock').read_bytes()),'toolchain_sha256':sha((dest/'rust-toolchain.toml').read_bytes())}
        write(root/'source-manifest.json',manifest)
        verify_exports(root)
        stage.record['manifest_sha256']=sha((root/'source-manifest.json').read_bytes())

if __name__=='__main__': main()
