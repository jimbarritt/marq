const MAPPING_FIXTURES = [
    {
        name: 'word in a paragraph',
        file: 'The build uses esbuild for bundling.\n',
        threads: [{ target: 'esbuild', expect: 'esbuild', flag: null }]
    },
    {
        name: 'repeated word, second occurrence',
        file: 'one two one two three\n',
        threads: [{ target: 'two', occurrence: 2, expect: 'two', before: 'one two one ' }]
    },
    {
        name: 'inside bold',
        file: 'a **bold word** here\n',
        threads: [
            { target: 'bold', expect: 'bold' },
            { target: '**bold word**', expect: 'bold word' }
        ]
    },
    {
        name: 'inside a code span',
        file: 'use `npm run build` now\n',
        threads: [{ target: 'run', expect: 'run' }]
    },
    {
        name: 'inside link text',
        file: 'see [the docs](http://example.com/docs) now\n',
        threads: [{ target: 'the docs', expect: 'the docs' }]
    },
    {
        name: 'in a heading',
        file: '# Title here\n\ntext\n',
        threads: [{ target: 'here', expect: 'here' }]
    },
    {
        name: 'in a list item',
        file: '- alpha\n- beta gamma\n',
        threads: [{ target: 'gamma', expect: 'gamma' }]
    },
    {
        name: 'in a nested list item',
        file: '- a\n  - nested item\n    lazy item\n',
        threads: [{ target: 'item', occurrence: 2, expect: 'item', before: 'nested item\nlazy ' }]
    },
    {
        name: 'in a task list item',
        file: '- [ ] task one\n- [x] done two\n',
        threads: [{ target: 'two', expect: 'two' }]
    },
    {
        name: 'in blockquote line 2',
        file: '> first line\n> second line\n',
        threads: [{ target: 'second', expect: 'second', before: 'first line\n' }]
    },
    {
        name: 'in a table cell',
        file: '| a | b \\| c |\n|---|---|\n| cell one | cell two |\n',
        threads: [
            { target: 'two', expect: 'two', before: 'cell one\ncell ' },
            { target: 'b \\| c', expect: 'b | c' }
        ]
    },
    {
        name: 'across two blocks',
        file: 'para one end\n\nsecond para start\n',
        threads: [{ target: 'end\n\nsecond', expect: 'endsecond' }]
    },
    {
        name: 'across emphasis boundaries',
        file: 'plain *emph text* more\n',
        threads: [{ target: 'ain *emph', expect: 'ain emph' }]
    },
    {
        name: 'entities',
        file: 'a &amp; b &fjlig; c &#x1F600; d &foo; e\n',
        threads: [
            { target: '&amp;', expect: '&' },
            { target: '&fjlig;', expect: 'fj' },
            { target: '&#x1F600;', expect: '\u{1F600}' },
            { target: '&foo;', expect: '&foo;' },
            { target: 'b &fjlig; c', expect: 'b fj c' },
            { target: 'amp', expect: '', flag: 'unaligned-block' }
        ]
    },
    {
        name: 'astral characters before the range',
        file: '\u{1F600}\u{1F600} emoji then target word\n',
        threads: [{ target: 'target', expect: 'target' }]
    },
    {
        name: 'CRLF source',
        file: 'line one\r\nline two target\r\n\r\nnext para\r\n',
        threads: [
            { target: 'target', expect: 'target' },
            { target: 'next', expect: 'next' }
        ]
    },
    {
        name: 'leading-tab code block',
        file: 'para\n\n\tcode target here\n',
        threads: [{ target: 'target', expect: 'target' }]
    },
    {
        name: 'byte order mark',
        file: '﻿hello world\n',
        bom: true,
        threads: [{ target: 'world', expect: 'world' }]
    },
    {
        name: 'image rewrite before and spanning the range',
        file: '![logo](img/logo.png) text target\n',
        rewrite: ['img/logo.png', 'file:///Users/example/docs/img/logo.png'],
        threads: [
            { target: 'target', expect: 'target' },
            { target: 'logo.png) text', expect: ' text' },
            { target: '![logo](img/logo.png) text', expect: ' text' }
        ]
    },
    {
        name: 'link reference definition before the range',
        file: '[ref]: http://example.com\n\nSee [ref] and target word\n',
        threads: [
            { target: 'target', expect: 'target' },
            { target: 'http://example.com', expect: '', flag: 'no-rendered-text' }
        ]
    },
    {
        name: 'unclosed div block',
        file: '<div align="center">\n\n# Title\n\nSome text target\n\nMore text\n',
        mapper: 'pairing',
        threads: [{ target: 'target', expect: '', flag: 'unaligned-block' }]
    },
    {
        name: 'paragraph merged with indented code',
        file: 'x\n    code\n---\n\nafter target\n',
        threads: [
            { target: 'code', expect: '', flag: 'unaligned-block' },
            { target: 'target', expect: 'target' }
        ]
    },
    {
        name: 'mermaid block',
        file: 'before\n\n```mermaid\ngraph TD\nA-->B\n```\n\nafter\n',
        threads: [
            { target: 'graph', expect: '', flag: 'typeset-block' },
            { target: 'after', expect: 'after' }
        ]
    },
    {
        name: 'KaTeX paragraph',
        file: 'Cost $x^2$ target here\n\nplain target\n',
        threads: [
            { target: 'target', expect: '', flag: 'typeset-block' },
            { target: 'target', occurrence: 2, expect: 'target' }
        ]
    },
    {
        name: 'syntax only',
        file: '# Title\n\n- item\n\n[a](http://dest.example.com)\n\n```js\ncode\n```\n',
        threads: [
            { target: '# ', expect: '', flag: 'no-rendered-text' },
            { target: '- ', expect: '', flag: 'no-rendered-text' },
            { target: 'http://dest.example.com', expect: '', flag: 'no-rendered-text' },
            { target: '```js', expect: '', flag: 'no-rendered-text' }
        ]
    },
    {
        name: 'changed anchor',
        file: 'The value is reloadingg now\n',
        threads: [{ target: 'reloadingg', status: 'changed', original: 'reloading', expect: 'reloadingg' }]
    },
    {
        name: 'orphaned and applied anchors',
        file: 'Some text here\n',
        threads: [
            { target: 'text', status: 'orphaned', expect: '' },
            { target: 'Some', status: 'applied', expect: '' }
        ]
    },
    {
        name: 'code block inside a list',
        file: '- item\n\n  ```\n  code target\n  ```\n',
        threads: [{ target: 'target', expect: 'target' }]
    },
    {
        name: 'source does not match md',
        file: 'The rendered text target\n',
        sourceOverride: 'A different file target\n',
        mapper: 'source-mismatch',
        threads: [{ target: 'target', expect: '', flag: 'source-mismatch' }]
    },
    {
        name: 'anchor text does not match the range',
        file: 'alpha beta gamma\n',
        threads: [{ target: 'beta', textOverride: 'BETA', expect: '', flag: 'source-mismatch' }]
    },
    {
        name: 'overlapping threads',
        file: 'one two three four\n',
        threads: [
            { target: 'two three', expect: 'two three' },
            { target: 'three four', expect: 'three four' }
        ]
    },
    {
        name: 'marks survive a search and its clearing',
        file: 'find the needle in the text\n',
        search: 'needle',
        threads: [{ target: 'needle in the', expect: 'needle in the' }]
    },
    {
        name: 'escaped punctuation',
        file: 'a \\*literal\\* star\n',
        threads: [{ target: '\\*literal\\*', expect: '*literal*' }]
    },
    {
        name: 'ordered list item whose text equals its marker',
        file: '1. 1\n2. 2\n',
        threads: [{ target: '1. ', expect: '', flag: 'no-rendered-text' }]
    }
];

if (typeof module !== 'undefined') module.exports = MAPPING_FIXTURES;
