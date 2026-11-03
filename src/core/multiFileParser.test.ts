/** @vitest-environment happy-dom */
import { describe, expect, it } from 'vitest';
import { strToU8, zipSync } from 'fflate';
import * as XLSX from 'xlsx';
import { parseLocalAttachment } from './multiFileParser';

const WORD_NS = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';

function makeDocx(documentXml: string): Uint8Array {
  return zipSync({
    '[Content_Types].xml': strToU8('<Types/>'),
    'word/document.xml': strToU8(documentXml),
  });
}

describe('DOCX WordprocessingML extraction', () => {
  it('supports alternate namespace prefixes and decodes XML entities', async () => {
    const xml = '<x:document xmlns:x="' + WORD_NS + '">' +
      '<x:body>\n' +
        '<x:p>\n' +
          '<x:r><x:t>A &amp; B &lt; C</x:t></x:r>\n' +
        '</x:p>\n' +
      '</x:body></x:document>';

    const parsed = await parseLocalAttachment('alternate-prefix.docx', makeDocx(xml));

    expect(parsed.markdownSummary).toBe('A & B < C');
  });

  it('preserves explicit spaces, tabs and line breaks in run order', async () => {
    const xml = '<w:document xmlns:w="' + WORD_NS + '" xmlns:xml="http://www.w3.org/XML/1998/namespace">' +
      '<w:body>' +
        '<w:p><w:r><w:t xml:space="preserve">  before </w:t><w:tab/><w:t>after</w:t><w:br/><w:t>next</w:t></w:r></w:p>' +
        '<w:p><w:r><w:t xml:space="preserve">  keep both sides  </w:t></w:r></w:p>' +
      '</w:body></w:document>';

    const parsed = await parseLocalAttachment('whitespace.docx', makeDocx(xml));

    expect(parsed.markdownSummary).toBe('  before \tafter\nnext\n\n  keep both sides  ');
  });

  it('preserves table rows and cells as Markdown with escaped cell separators', async () => {
    const xml = '<d:document xmlns:d="' + WORD_NS + '">' +
      '<d:body><d:tbl>' +
        '<d:tr>' +
          '<d:tc><d:p><d:r><d:t>Name</d:t></d:r></d:p></d:tc>' +
          '<d:tc><d:p><d:r><d:t>Value</d:t></d:r></d:p></d:tc>' +
        '</d:tr>' +
        '<d:tr>' +
          '<d:tc><d:p><d:r><d:t>A | B</d:t></d:r></d:p></d:tc>' +
          '<d:tc><d:p><d:r><d:t>Line 1</d:t><d:br/><d:t>Line 2</d:t></d:r></d:p></d:tc>' +
        '</d:tr>' +
      '</d:tbl></d:body></d:document>';

    const parsed = await parseLocalAttachment('table.docx', makeDocx(xml));

    expect(parsed.markdownSummary).toBe(
      '| Name | Value |\n| --- | --- |\n| A \\| B | Line 1<br>Line 2 |',
    );
  });

  it('reports malformed document XML as a DOCX parsing error', async () => {
    const parsed = await parseLocalAttachment(
      'malformed.docx',
      makeDocx('<w:document><w:body><w:p></w:body></w:document>'),
    );

    expect(parsed.markdownSummary).toMatch(/^\[DOCX 파싱 실패:/);
  });
  it('keeps XLSX workbook import behavior with the maintained SheetJS release', async () => {
    const workbook = XLSX.utils.book_new();
    const sheet = XLSX.utils.aoa_to_sheet([
      ['Name', 'Score'],
      ['Student A', 95],
    ]);
    XLSX.utils.book_append_sheet(workbook, sheet, 'Scores');
    const bytes = XLSX.write(workbook, { type: 'array', bookType: 'xlsx' });

    const parsed = await parseLocalAttachment('scores.xlsx', new Uint8Array(bytes));

    expect(parsed.kind).toBe('excel');
    expect(parsed.markdownSummary).toBe(
      '### [시트: Scores]\n| Name | Score |\n| --- | --- |\n| Student A | 95 |',
    );
    expect(parsed.structuredData).toEqual({
      Scores: [['Name', 'Score'], ['Student A', 95]],
    });
  });

});
