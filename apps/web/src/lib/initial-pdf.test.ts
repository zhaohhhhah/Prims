import {describe, expect, it} from 'vitest';
import {initialPdfPollInterval, isCompilePending, TUTORIAL_PROJECT_NAME as tutorial} from './initial-pdf';
describe('first tutorial PDF preparation', () => {
  it('waits briefly for the asynchronous producer to create its first job', () => {
    expect(initialPdfPollInterval(tutorial, false, undefined, 200)).toBe(2000);
    expect(initialPdfPollInterval(tutorial, false, undefined, 30_000)).toBe(false);
  });
  it('follows queued and running jobs with a fixed maximum window', () => {
    for (const status of ['queued','running']) {
      expect(isCompilePending({status})).toBe(true);
      expect(initialPdfPollInterval(tutorial, false, {status}, 40_000)).toBe(2000);
      expect(initialPdfPollInterval(tutorial, false, {status}, 180_000)).toBe(false);
    }
  });
  it('never repeatedly renders a failed or cancelled document', () => {
    for (const status of ['error','cancelled','success']) {
      expect(isCompilePending({status})).toBe(false);
      expect(initialPdfPollInterval(tutorial, false, {status}, 1000)).toBe(false);
    }
  });
  it('leaves ordinary projects and existing PDFs idle', () => {
    expect(initialPdfPollInterval('My paper', false, {status:'running'}, 1000)).toBe(false);
    expect(initialPdfPollInterval(tutorial, true, {status:'running'}, 1000)).toBe(false);
  });
});
