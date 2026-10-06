import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const macos = resolve(here, '../..');
const repo = resolve(macos, '..');
const resources = join(macos, 'Sources/marq/Resources');
const chrome = process.env.CHROME_BIN || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const fuzzCount = Number(process.env.MAPPING_FUZZ ?? 3000);
const fuzzSeed = Number(process.env.MAPPING_SEED ?? 20261006);

const skipDirs = new Set(['.git', '.build', 'node_modules', 'target', '.harness', 'build']);

function markdownFiles(dir) {
    const out = [];
    for (const name of readdirSync(dir)) {
        if (skipDirs.has(name)) continue;
        const path = join(dir, name);
        const info = statSync(path);
        if (info.isDirectory()) out.push(...markdownFiles(path));
        else if (name.endsWith('.md')) out.push(path);
    }
    return out;
}

function inlineScript(code) {
    return '<script>' + code.replace(/<\/(script)/gi, '<\\/$1') + '</script>';
}

const corpus = markdownFiles(repo).map((path) => readFileSync(path, 'utf8'));
const template = readFileSync(join(resources, 'template.html'), 'utf8');
const page = template
    .replace('<head>', '<head>\n<base href="' + pathToFileURL(resources).href + '/">')
    .replace('</body>', [
        '<pre id="mapping-results"></pre>',
        inlineScript('window.MAPPING_CORPUS = ' + JSON.stringify(corpus) + ';'
            + 'window.MAPPING_FUZZ_COUNT = ' + fuzzCount + ';'
            + 'window.MAPPING_FUZZ_SEED = ' + fuzzSeed + ';'),
        inlineScript(readFileSync(join(here, 'fixtures.js'), 'utf8')),
        inlineScript(readFileSync(join(here, 'harness.js'), 'utf8')),
        '</body>'
    ].join('\n'));

const work = mkdtempSync(join(tmpdir(), 'marq-mapping-'));
const pagePath = join(work, 'page.html');
writeFileSync(pagePath, page);

async function waitFor(check, limitMs, what) {
    const deadline = Date.now() + limitMs;
    for (;;) {
        const value = await check();
        if (value) return value;
        if (Date.now() > deadline) throw new Error('timed out waiting for ' + what);
        await new Promise((r) => setTimeout(r, 200));
    }
}

async function runInChrome(url) {
    const profile = join(work, 'profile');
    const child = spawn(chrome, [
        '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
        '--allow-file-access-from-files', '--user-data-dir=' + profile,
        '--window-size=1200,900', '--remote-debugging-port=0', 'about:blank'
    ], { stdio: 'ignore' });
    try {
        const portFile = join(profile, 'DevToolsActivePort');
        const port = await waitFor(() => existsSync(portFile) && readFileSync(portFile, 'utf8').split('\n')[0], 30000, 'Chrome to start');
        const targets = await waitFor(async () => {
            const list = await fetch('http://127.0.0.1:' + port + '/json/list').then((r) => r.json(), () => []);
            return list.find((t) => t.type === 'page') && list;
        }, 30000, 'a page target');
        const socket = new WebSocket(targets.find((t) => t.type === 'page').webSocketDebuggerUrl);
        await new Promise((ok, fail) => { socket.onopen = ok; socket.onerror = fail; });
        let nextId = 1;
        const pending = new Map();
        socket.onmessage = (event) => {
            const message = JSON.parse(event.data);
            if (message.id && pending.has(message.id)) {
                pending.get(message.id)(message);
                pending.delete(message.id);
            }
        };
        const send = (method, params) => new Promise((ok) => {
            const id = nextId++;
            pending.set(id, ok);
            socket.send(JSON.stringify({ id, method, params: params || {} }));
        });
        await send('Page.navigate', { url });
        const text = await waitFor(async () => {
            const reply = await send('Runtime.evaluate', {
                expression: "(document.getElementById('mapping-results') || {}).textContent || ''",
                returnByValue: true
            });
            return reply.result && reply.result.result && reply.result.result.value;
        }, 600000, 'the harness results');
        socket.close();
        return text;
    } finally {
        child.kill('SIGKILL');
    }
}

const results = JSON.parse(await runInChrome(pathToFileURL(pagePath).href));

let failed = 0;
for (const f of results.fixtures) {
    console.log((f.pass ? 'PASS ' : 'FAIL ') + f.name);
    if (!f.pass) {
        failed++;
        for (const line of f.failures) console.log('     ' + line);
    }
}
for (const e of results.errors) {
    failed++;
    console.log('ERROR ' + e);
}

if (results.fuzz) {
    const z = results.fuzz;
    console.log(`fuzz: ${z.cases} cases (seed ${fuzzSeed}, ${z.generated} generated, the rest sliced from the repository's .md files), ${z.checked} checked against the sentinel oracle, `
        + `${z.oracleSkipped} without a valid oracle, ${z.liveDiffers} typeset, ${z.mapperNotOk} mapper not ok`);
    console.log(`fuzz: ${z.withMarks} cases marked text, ${z.exact} marked exactly the oracle's text`);
    console.log(`fuzz: ${z.violations.length} marked text outside the range, ${z.threw.length} threw`);
    if (process.env.MAPPING_VERBOSE) for (const v of z.inexact) console.log('     inexact ' + JSON.stringify(v));
    for (const v of z.violations) console.log('     violation ' + JSON.stringify(v));
    for (const t of z.threw) console.log('     threw ' + JSON.stringify(t));
    if (z.violations.length || z.threw.length) failed++;
}

console.log(`${results.fixtures.length - results.fixtures.filter((f) => !f.pass).length}/${results.fixtures.length} fixtures pass`);
process.exit(failed ? 1 : 0);
