import { describe, expect, it } from 'vitest';
import { PrivacySessionMasker } from './privacyMasker';

describe('PrivacySessionMasker account number masking', () => {
  it('masks a Korean account number when it is labeled and restores it locally', () => {
    const masker = new PrivacySessionMasker();
    const original = '입금계좌번호: 110-123-456789';

    const masked = masker.maskTextForPrompt(original, [], true);

    expect(masked).not.toContain('110-123-456789');
    expect(masked).toMatch(/\[\[PII_ACCOUNT_\d+\]\]/);
    expect(masker.unmaskText(masked)).toBe(original);
  });

  it('masks a continuous account number when it has an explicit account label', () => {
    const masker = new PrivacySessionMasker();
    const original = '계좌 123456789012';

    const masked = masker.maskTextForPrompt(original, [], true);

    expect(masked).not.toContain('123456789012');
    expect(masker.unmaskText(masked)).toBe(original);
  });

  it('does not mask unrelated numeric identifiers without an account label', () => {
    const masker = new PrivacySessionMasker();
    const original = '학번 110-123-456789, 문서번호 123456789012';

    expect(masker.maskTextForPrompt(original, [], true)).toBe(original);
  });

  it('leaves account numbers unchanged when automatic PII masking is disabled', () => {
    const masker = new PrivacySessionMasker();
    const original = '계좌번호: 110-123-456789';

    expect(masker.maskTextForPrompt(original, [], false)).toBe(original);
  });
});
