(function() {
    const results = { fixtures: [], fuzz: null, errors: [] };

    function codePoints(text) {
        return Array.from(text).length;
    }

    function nthIndex(haystack, needle, n) {
        let at = -1;
        for (let i = 0; i < n; i++) {
            at = haystack.indexOf(needle, at + 1);
            if (at < 0) return -1;
        }
        return at;
    }

    function applyEditList(text, edits) {
        let out = '';
        let last = 0;
        edits.slice().sort(function(a, b) { return a[0] - b[0]; }).forEach(function(e) {
            out += text.slice(last, e[0]) + e[2];
            last = e[0] + e[1];
        });
        return out + text.slice(last);
    }

    function gutterEntries() {
        const content = document.getElementById('content');
        const top = content.getBoundingClientRect().top;
        return Array.from(content.querySelectorAll(':scope > [data-source-line]')).map(function(el) {
            return [el.getAttribute('data-source-line'), el.getBoundingClientRect().top - top];
        });
    }

    function liveText(root) {
        const nodes = commentTextNodes(Array.from(root.childNodes));
        return nodes;
    }

    function markedIndices(root) {
        const nodes = liveText(root);
        let text = '';
        const marked = [];
        nodes.forEach(function(node) {
            const inMark = !!(node.parentElement && node.parentElement.closest('mark.cm'));
            for (let i = 0; i < node.data.length; i++) if (inMark) marked.push(text.length + i);
            text += node.data;
        });
        return { text: text, marked: marked };
    }

    function textBeforeFirstMark(root, id) {
        const nodes = liveText(root);
        let text = '';
        for (let i = 0; i < nodes.length; i++) {
            const mark = nodes[i].parentElement && nodes[i].parentElement.closest('mark.cm');
            if (mark && mark.closest('mark.cm[data-thread="' + id + '"]')) return text;
            text += nodes[i].data;
        }
        return null;
    }

    function buildCase(fx, index) {
        const file = fx.file;
        const withoutBom = fx.bom ? file.slice(1) : file;
        const edits = [];
        if (fx.rewrite) {
            let at = withoutBom.indexOf(fx.rewrite[0]);
            while (at >= 0) {
                edits.push([at, fx.rewrite[0].length, fx.rewrite[1]]);
                at = withoutBom.indexOf(fx.rewrite[0], at + fx.rewrite[0].length);
            }
        }
        const md = applyEditList(withoutBom, edits).replace(/\r\n|\r/g, '\n');
        const threads = fx.threads.map(function(t, k) {
            const at = nthIndex(file, t.target, t.occurrence || 1);
            if (at < 0) throw new Error('target not found: ' + t.target);
            const start = codePoints(file.slice(0, at));
            const end = start + codePoints(t.target);
            const status = t.status || 'anchored';
            let anchor;
            if (status === 'anchored' || status === 'changed') {
                anchor = { status: status, start: start, end: end, line: 1, column: 1, text: t.textOverride || t.target };
                if (status === 'changed') anchor.original = t.original;
            } else {
                anchor = { status: status };
            }
            return {
                annotation: { id: 'urn:uuid:fixture-' + index + '-' + k, type: 'Annotation' },
                state: 'open',
                anchor: anchor,
                stateChanges: [],
                replies: []
            };
        });
        const payload = {
            threads: threads,
            source: fx.sourceOverride || withoutBom,
            bom: !!fx.bom,
            edits: edits
        };
        return { md: md, payload: payload };
    }

    function checkEntries(fx, label, failures) {
        const content = document.getElementById('content');
        fx.threads.forEach(function(t, k) {
            const entry = commentsModel.entries[k];
            const where = label + ' thread ' + k + ' (' + JSON.stringify(t.target) + ')';
            if (entry.markedText !== t.expect) {
                failures.push(where + ': markedText ' + JSON.stringify(entry.markedText) + ', expected ' + JSON.stringify(t.expect) + ', flag ' + entry.flag + ', mapper ' + commentsState.mapper);
            }
            const domMarks = Array.from(content.querySelectorAll('mark.cm')).filter(function(m) {
                return m.getAttribute('data-thread') === entry.id;
            });
            if (t.expect === '' && domMarks.length) failures.push(where + ': expected no marks, found ' + domMarks.length);
            if (domMarks.length !== entry.marks.length) failures.push(where + ': model marks differ from the DOM');
            if ('flag' in t && entry.flag !== t.flag) {
                failures.push(where + ': flag ' + JSON.stringify(entry.flag) + ', expected ' + JSON.stringify(t.flag));
            }
            if (t.before) {
                const before = textBeforeFirstMark(content, entry.id);
                if (before === null || !before.endsWith(t.before)) {
                    failures.push(where + ': text before the mark ' + JSON.stringify(before && before.slice(-30)) + ' does not end with ' + JSON.stringify(t.before));
                }
            }
        });
        const expectedMapper = fx.mapper || 'ok';
        if (commentsState.mapper !== expectedMapper) {
            failures.push(label + ': mapper ' + commentsState.mapper + ', expected ' + expectedMapper);
        }
    }

    function runFixture(fx, index) {
        const failures = [];
        const built = buildCase(fx, index);
        commentsModel = null;
        renderMarkdown(built.md, true);
        const content = document.getElementById('content');
        const plainHtml = content.innerHTML;
        const gutterPlain = JSON.stringify(gutterEntries());

        applyComments(JSON.stringify({ threads: [], source: built.payload.source, bom: built.payload.bom, edits: built.payload.edits }));
        if (content.innerHTML !== plainHtml) failures.push('a payload with no threads changed the DOM');

        applyComments(JSON.stringify(built.payload));
        checkEntries(fx, 'painted', failures);
        if (JSON.stringify(gutterEntries()) !== gutterPlain) failures.push('gutter entries differ with marks painted');

        paintComments();
        checkEntries(fx, 'repainted', failures);

        if (fx.search) {
            performSearch(fx.search);
            if (!content.querySelector('mark.search-highlight')) failures.push('search found nothing');
            checkEntries(fx, 'during search', failures);
            clearSearch();
            checkEntries(fx, 'after search', failures);
        }

        commentsModel = null;
        paintComments();
        if (content.querySelector('mark.cm')) failures.push('marks left after the model was cleared');
        if (content.innerHTML !== plainHtml && !fx.search) failures.push('unpainting did not restore the plain DOM');

        results.fixtures.push({ name: fx.name, pass: failures.length === 0, failures: failures });
    }

    function mulberry32(seed) {
        return function() {
            seed |= 0;
            seed = seed + 0x6D2B79F5 | 0;
            let t = Math.imul(seed ^ seed >>> 15, 1 | seed);
            t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
            return ((t ^ t >>> 14) >>> 0) / 4294967296;
        };
    }

    function detachedText(md) {
        const div = document.createElement('div');
        div.innerHTML = renderWithLineNumbers(md);
        return commentTextNodes(Array.from(div.childNodes)).map(function(n) { return n.data; }).join('');
    }

    function generatedDocument(random) {
        const pick = function(list) { return list[Math.floor(random() * list.length)]; };
        const words = ['alpha', 'beta', 'gamma', 'one', 'two', 'x', '1', 'a', 'the', 'café', '\u{1F600}'];
        const inline = function() {
            const parts = [];
            const n = 1 + Math.floor(random() * 6);
            for (let i = 0; i < n; i++) {
                const w = pick(words);
                parts.push(pick([w, w, w, '*' + w + '*', '**' + w + '**', '`' + w + '`', '[' + w + '](http://e.com/' + w + ')',
                    '&amp;', '&fjlig;', '&#x1F600;', '&foo;', '\\*', '~~' + w + '~~', '<b>' + w + '</b>', w + '  \n' + w,
                    '![' + w + '](img/' + w + '.png)', '[' + w + ']', w + '\t' + w]));
            }
            return parts.join(' ');
        };
        const block = function(depth) {
            const kind = Math.floor(random() * (depth > 1 ? 6 : 11));
            switch (kind) {
            case 0: case 1: case 2: return inline();
            case 3: return pick(['#', '##', '###']) + ' ' + inline();
            case 4: return '```' + pick(['', 'js', 'mermaid']) + '\n' + inline() + '\n' + inline() + '\n```';
            case 5: return pick(['    ', '\t']) + inline() + '\n' + pick(['    ', '\t']) + inline();
            case 6: return block(depth + 1).split('\n').map(function(l) { return pick(['> ', '>', '> > ']) + l; }).join('\n');
            case 7: {
                const marker = pick(['-', '*', '1.', '2)']);
                return [0, 1, 2].slice(0, 1 + Math.floor(random() * 3)).map(function() {
                    const body = block(depth + 1).split('\n');
                    const indent = ' '.repeat(marker.length + 1);
                    return marker + ' ' + pick(['', '', '[ ] ', '[x] ']) + body[0]
                        + body.slice(1).map(function(l) { return '\n' + pick([indent, indent, '\t', '']) + l; }).join('');
                }).join(pick(['\n', '\n\n']));
            }
            case 8: {
                const cols = 1 + Math.floor(random() * 3);
                const row = function() {
                    const cells = [];
                    for (let c = 0; c < cols; c++) cells.push(pick([inline(), pick(words) + ' \\| ' + pick(words), '']));
                    return '| ' + cells.join(' | ') + ' |';
                };
                return [row(), '|' + '---|'.repeat(cols), row(), row()].join('\n');
            }
            case 9: return '[' + pick(words) + ']: http://e.com/' + pick(words) + '\n\n' + inline();
            default: return pick(['---', '<div>\n' + inline() + '\n</div>', inline() + '\n    ' + inline() + '\n---', '$x^2$ ' + inline()]);
            }
        };
        const blocks = [];
        const n = 1 + Math.floor(random() * 5);
        for (let i = 0; i < n; i++) blocks.push(block(0));
        return blocks.join(pick(['\n\n', '\n\n', '\n'])) + '\n';
    }

    function runFuzz(corpus, count, seed) {
        const random = mulberry32(seed);
        const stats = { cases: 0, checked: 0, oracleSkipped: 0, liveDiffers: 0, violations: [], threw: [],
                        withMarks: 0, exact: 0, mapperNotOk: 0, generated: 0, inexact: [] };
        const content = document.getElementById('content');
        for (let n = 0; n < count; n++) {
            let md;
            if (n % 2) {
                md = generatedDocument(random);
                stats.generated++;
            } else {
                const doc = corpus[Math.floor(random() * corpus.length)];
                const lines = doc.split('\n');
                const from = Math.floor(random() * lines.length);
                const span = 1 + Math.floor(random() * 40);
                md = lines.slice(from, from + span).join('\n') + '\n';
            }
            const cps = Array.from(md);
            if (cps.length < 2) continue;
            const a = Math.floor(random() * (cps.length - 1));
            const b = Math.min(cps.length, a + 1 + Math.floor(random() * 60));
            const crlf = random() < 0.2;
            const bom = random() < 0.1;
            const prefix = cps.slice(0, a).join('');
            const target = cps.slice(a, b).join('');
            const fileBody = crlf ? md.replace(/\n/g, '\r\n') : md;
            const filePrefix = crlf ? prefix.replace(/\n/g, '\r\n') : prefix;
            const fileTarget = crlf ? target.replace(/\n/g, '\r\n') : target;
            const start = codePoints(filePrefix) + (bom ? 1 : 0);
            const end = start + codePoints(fileTarget);
            const payload = {
                threads: [{
                    annotation: { id: 'urn:uuid:fuzz-' + n },
                    state: 'open',
                    anchor: { status: 'anchored', start: start, end: end, line: 1, column: 1, text: fileTarget },
                    stateChanges: [],
                    replies: []
                }],
                source: fileBody,
                bom: bom,
                edits: []
            };
            stats.cases++;
            try {
                commentsModel = null;
                renderMarkdown(md, true);
                applyComments(JSON.stringify(payload));
            } catch (e) {
                stats.threw.push({ n: n, error: String(e && e.stack || e), md: md, start: start, end: end });
                continue;
            }
            if (commentsState.mapper !== 'ok') stats.mapperNotOk++;
            const plain = detachedText(md);
            const sentinel = detachedText(prefix + '' + target + '' + cps.slice(b).join(''));
            const s0 = sentinel.indexOf(''), s1 = sentinel.indexOf('');
            const valid = s0 >= 0 && s1 > s0
                && sentinel.indexOf('', s0 + 1) < 0 && sentinel.indexOf('', s1 + 1) < 0
                && sentinel.replace(/[]/g, '') === plain;
            if (!valid) { stats.oracleSkipped++; continue; }
            const live = markedIndices(content);
            if (live.text !== plain) {
                stats.liveDiffers++;
                if (live.marked.length) {
                    const thread = commentsModel.entries[0];
                    if (thread.flag !== 'partly-marked' && thread.flag !== null) {
                        stats.violations.push({ n: n, reason: 'marks with flag ' + thread.flag, md: md, target: target });
                    }
                }
                continue;
            }
            stats.checked++;
            const lo = s0, hi = s1 - 1;
            const outside = live.marked.filter(function(i) { return i < lo || i >= hi; });
            if (outside.length) {
                stats.violations.push({
                    n: n, md: md, target: target,
                    marked: live.marked.map(function(i) { return live.text[i]; }).join(''),
                    oracle: plain.slice(lo, hi)
                });
            }
            if (live.marked.length) stats.withMarks++;
            const oracleChars = plain.slice(lo, hi).replace(/\s+/g, '');
            const markedChars = live.marked.map(function(i) { return live.text[i]; }).join('').replace(/\s+/g, '');
            if (oracleChars && oracleChars === markedChars) stats.exact++;
            else if (stats.inexact.length < 40) stats.inexact.push({ md: md, target: target, marked: markedChars, oracle: oracleChars, flag: commentsModel.entries[0].flag });
        }
        stats.violations = stats.violations.slice(0, 20);
        stats.threw = stats.threw.slice(0, 20);
        return stats;
    }

    try {
        MAPPING_FIXTURES.forEach(function(fx, i) {
            try {
                runFixture(fx, i);
            } catch (e) {
                results.fixtures.push({ name: fx.name, pass: false, failures: ['threw: ' + (e && e.stack || e)] });
            }
        });
        if (window.MAPPING_FUZZ_COUNT > 0) {
            results.fuzz = runFuzz(window.MAPPING_CORPUS, window.MAPPING_FUZZ_COUNT, window.MAPPING_FUZZ_SEED);
        }
    } catch (e) {
        results.errors.push(String(e && e.stack || e));
    }
    document.getElementById('mapping-results').textContent = JSON.stringify(results);
})();
