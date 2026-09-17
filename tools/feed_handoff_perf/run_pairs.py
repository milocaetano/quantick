"""One authorized paired attempt, no internal retries or discarded samples."""
from pathlib import Path
import argparse,datetime,hashlib,json,os,subprocess

import host
from analysis import summarize
from source import verify_binary_identity
p=argparse.ArgumentParser()
p.add_argument('--lease',required=True,help='Coordinator quiet-host authorization reference')
p.add_argument('--control',required=True)
p.add_argument('--candidate',required=True)
p.add_argument('--output',required=True)
p.add_argument('--build-identity',required=True,help='Recorded compiler/profile/jobs/commands for both exact binaries')
p.add_argument('--source-manifest',required=True)
a=p.parse_args()
out=Path(a.output)
if out.exists():raise SystemExit('Refuse to reuse attempt directory')
out.mkdir(parents=True)
executables={side:Path(getattr(a,side)).resolve() for side in ['control','candidate']}
build=json.loads(Path(a.build_identity).read_text(encoding='utf-8'))
verify_binary_identity(build,executables,lambda path:hashlib.sha256(path.read_bytes()).hexdigest())
system=host.system_identity()
metadata={'lease':a.lease,'start_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'executables':{side:{'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()} for side,path in executables.items()},
    'source_manifest_sha256':hashlib.sha256(Path(a.source_manifest).read_bytes()).hexdigest(),
    'statistics':'sample CV (ddof=1), median, nearest-rank p95 (15 samples -> largest), no trimming',
    'build_identity':build,'system':system,
    'runtime_rustc':subprocess.check_output(['rustc','-Vv']).decode(),
    'runtime_cargo':subprocess.check_output(['cargo','-V']).decode(),
    'relevant_environment':{key:value for key,value in os.environ.items() if key in ['RUSTFLAGS','CARGO_BUILD_JOBS','QUANTICK_BACKFILL'] or key.startswith('CARGO_PROFILE_')}}
(out/'identity.json').write_text(json.dumps(metadata,indent=2)+'\n',encoding='utf-8')
def inventory(name):
    known={tuple(identity) for side in ['control','candidate'] for identity in build[side]['observed_build_descendants']}
    receipt=host.observe(known)
    (out/name).write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
    if receipt['blockers']:
        (out/'stopped-host.json').write_text(json.dumps({'stage':name,'reason':'host admission refused','blockers':receipt['blockers']},indent=2)+'\n',encoding='utf-8')
        raise SystemExit('Host admission refused; observations retained; no retry')
inventory('process-before.json')
records=[]
for case in ['binance','dense','exclusion']:
    for phase,count in [('warmup',3),('measured',15)]:
        for pair in range(count):
            order=['control','candidate'] if pair%2==0 else ['candidate','control']
            observed={}
            for side in order:
                tag=f'{case}-{phase}-{pair+1:02}-{side}'
                env=dict(os.environ,F2_BENCH_CASE=case)
                command=[str(executables[side]),'f2_protocol::f2_protocol_sample','--ignored','--exact','--nocapture']
                started=datetime.datetime.now(datetime.timezone.utc).isoformat()
                try:
                    result=subprocess.run(command,env=env,capture_output=True,timeout=360)
                except subprocess.TimeoutExpired as error:
                    (out/(tag+'.log')).write_bytes((error.stdout or b'')+(error.stderr or b''))
                    (out/'stopped.json').write_text(json.dumps({'tag':tag,'reason':'timeout','start':started}),encoding='utf-8')
                    inventory('process-after.json');raise
                raw=result.stdout+result.stderr
                (out/(tag+'.log')).write_bytes(raw)
                values=[json.loads(line[len('F2_SAMPLE '):]) for line in raw.decode(errors='replace').splitlines() if line.startswith('F2_SAMPLE ')]
                if len(values)==1:
                    sample=values[0]
                    sample['costs']={'ns_per_live_trade':sample['elapsed_ns']/1_000_000} if case=='binance' else {'ns_per_batch':sample['elapsed_ns']/10_000,'ns_per_input_row':sample['elapsed_ns']/40_000}
                record={'case':case,'phase':phase,'pair':pair+1,'side':side,'start_utc':started,'finish_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'exit':result.returncode,'raw_sha256':hashlib.sha256(raw).hexdigest(),'sample':values[0] if len(values)==1 else None}
                records.append(record)
                with (out/'samples.jsonl').open('a',encoding='utf-8') as f:f.write(json.dumps(record)+'\n')
                print(json.dumps(record),flush=True)
                if result.returncode or len(values)!=1:
                    inventory('process-after.json');raise SystemExit('FAILED execution; retained; no retry')
                observed[side]=values[0]['totals']
            for key in ['trades','checksum']:
                if observed['control'][key]!=observed['candidate'][key]:
                    inventory('process-after.json');raise SystemExit('FAILED usable-trade parity; retained; no retry')
inventory('process-after.json')
summary=summarize(records)
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8')
print(json.dumps(summary),flush=True)
raise SystemExit(0 if all(x['verdict']=='PASS' for x in summary.values()) else 1)
