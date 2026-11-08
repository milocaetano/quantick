import { PineTS } from 'pinets';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../../', import.meta.url));
const fixtures = root + 'crates/pine/tests/fixtures/oracle/';
const golden = root + 'crates/pine/tests/golden/oracle/';
for (const tape of ['time', 'tick', 'volume']) {
    const rows = readFileSync(fixtures + tape + '.csv', 'utf8').trim().split('\n').slice(1);
    const bars = rows.map(row => {
        const [openTime, closeTime, open, high, low, close, buy, sell, trades] = row.split(',').map(Number);
        return { openTime, closeTime, open, high, low, close, volume: buy + sell, trades };
    });
    for (const name of ['sma', 'ema', 'rsi', 'atr', 'macd', 'stdev', 'crossover', 'pivothigh']) {
        const source = readFileSync(fixtures + name + '.pine', 'utf8');
        const { plots } = await new PineTS(bars).run(source);
        const titles = Object.keys(plots).filter(title => !title.startsWith('__'));
        const output = ['# pinets=0.13.1 format=1', titles.join(',')];
        for (let i = 0; i < bars.length; i++) {
            output.push(titles.map(title => {
                const value = plots[title].data[i]?.value;
                if (typeof value !== 'number') throw new Error(`${name}/${title}/${i}: missing numeric output`);
                return Number.isNaN(value) ? 'na' : value.toFixed(9);
            }).join(','));
        }
        writeFileSync(golden + `${tape}-${name}.csv`, output.join('\n') + '\n');
    }
}

