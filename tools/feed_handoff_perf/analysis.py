"""Frozen protocol statistics; every measured sample participates."""
import math
import statistics


def summarize(records):
    summary = {}
    for case in ['binance', 'dense', 'exclusion']:
        stats = {}
        for side in ['control', 'candidate']:
            values = [x['sample']['elapsed_ns'] for x in records
                      if x['case'] == case and x['phase'] == 'measured' and x['side'] == side]
            assert len(values) == 15
            stats[side] = {'median_ns': statistics.median(values),
                           'p95_ns': sorted(values)[math.ceil(.95*len(values))-1],
                           'cv': statistics.stdev(values)/statistics.mean(values)}
        med = stats['candidate']['median_ns']/stats['control']['median_ns']
        p95 = stats['candidate']['p95_ns']/stats['control']['p95_ns']
        verdict = ('INVALID_NOISE' if max(stats[s]['cv'] for s in stats) > .05
                   else 'PASS' if med <= 1.05 and p95 <= 1.10 else 'FAIL')
        summary[case] = {'stats': stats, 'median_ratio': med, 'p95_ratio': p95, 'verdict': verdict,
                         'median_label': 'better' if med < .95 else 'flat/noise' if med <= 1.05 else 'regression'}
    return summary
