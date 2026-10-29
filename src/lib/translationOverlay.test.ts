import {describe, expect, it} from 'vitest';
import {previewFixture} from '../../tests/overlay/preview-fixture';
import {mergeTranslation, previewSubtitles} from './translationOverlay';

describe('translations drawn on the matching captured image', () => {
  it('uses current OCR coordinates and keeps complete translated text', () => {
    const {frame, ocr, translation} = previewFixture();
    translation.results[0].sourceBlocks[0].bbox = [0, 0, 2, 2];
    const items = previewSubtitles(frame, ocr, translation);
    expect(items.map(item => item.text)).toEqual(['新游戏', '加载游戏']);
    expect(items[0].bbox).toEqual([400, 200, 700, 260]);
  });
  it('rejects other frames, capture sessions, models and rerun source text', () => {
    for (const mutate of [
      (f: ReturnType<typeof previewFixture>) => f.frame.frameId++,
      (f: ReturnType<typeof previewFixture>) => f.frame.generation++,
      (f: ReturnType<typeof previewFixture>) => f.frame.width++,
      (f: ReturnType<typeof previewFixture>) => f.ocr.modelId = 'other-model',
      (f: ReturnType<typeof previewFixture>) => f.ocr.blocks.forEach(b => b.text = 'Changed text'),
    ]) {
      const f = previewFixture(); mutate(f);
      expect(previewSubtitles(f.frame, f.ocr, f.translation)).toEqual([]);
    }
  });
  it('filters partial, cancelled, untranslated and mismatched revisions', () => {
    for (const change of [
      {status: 'incomplete'}, {status: 'error'}, {truncated: true}, {error: 'failure'},
      {stopReason: 'cancelled'}, {stopReason: 'skip'}, {translatedText: ' '},
      {sourceLanguage: 'zh'}, {sourceLanguage: 'none'}, {translatedText: 'New Game'},
      {revision: 4}, {epoch: 1}, {sourceBlockIds: ['unknown']},
    ]) {
      const {frame, ocr, translation} = previewFixture();
      translation.results = [Object.assign(translation.results[0], change)];
      expect(previewSubtitles(frame, ocr, translation)).toEqual([]);
    }
  });
  it('supports adjacent multi-block text but rejects wide groups and duplicate IDs', () => {
    const {frame, ocr, translation} = previewFixture();
    translation.results = [{...translation.results[0], sourceBlockIds: ['a', 'b'], sourceBlocks: structuredClone(ocr.blocks), translatedText: '新游戏或加载游戏'}];
    expect(previewSubtitles(frame, ocr, translation)[0].bbox).toEqual([400, 200, 700, 380]);
    ocr.blocks[0].bbox = [0, 0, 20, 20]; ocr.blocks[1].bbox = [1200, 600, 1220, 620];
    expect(previewSubtitles(frame, ocr, translation)).toEqual([]);
    translation.results[0].sourceBlockIds = ['a', 'a'];
    expect(previewSubtitles(frame, ocr, translation)).toEqual([]);
  });
  it('prevents an out-of-order translation from replacing the newer revision', () => {
    const {translation} = previewFixture();
    const events = [translation];
    expect(mergeTranslation(events, {...translation, revision: 4})).toBe(events);
    expect(mergeTranslation(events, {...translation, epoch: 1, revision: 99})).toBe(events);
    const next = {...translation, revision: 6, results: []};
    expect(mergeTranslation(events, next)).toEqual([next]);
  });
});
