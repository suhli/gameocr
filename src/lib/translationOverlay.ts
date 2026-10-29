import type {OcrResult, Preview} from '../types';
import type {OverlayItem} from '../types/overlay';
import type {TranslationEvent} from '../types/translation';
import {unionBounds} from './overlayGeometry';

const normalize = (text: string) => text.replace(/\s+/gu, ' ').trim();

/** A preview owns its image; never put another frame's translations on it. */
export function previewSubtitles(
  frame: Preview | null,
  ocr: OcrResult | null,
  event: TranslationEvent | null,
): OverlayItem[] {
  if (!frame || !ocr || !event || event.frameId !== frame.frameId
    || event.frameId !== ocr.frameId || event.generation !== frame.generation
    || frame.width !== ocr.sourceWidth || frame.height !== ocr.sourceHeight) return [];
  const items: OverlayItem[] = [], seen = new Set<string>();
  for (const result of event.results) {
    const text = result.translatedText.trim();
    if (result.frameId !== event.frameId || result.generation !== event.generation
      || result.epoch !== event.epoch || result.revision !== event.revision
      || result.sourceModelId !== ocr.modelId || result.status !== 'complete'
      || result.truncated || result.error != null || ['skip', 'cancelled'].includes(result.stopReason)
      || result.sourceLanguage === 'none' || result.sourceLanguage === result.targetLanguage
      || !text || Array.from(text).length > 8192 || normalize(text) === normalize(result.sourceText)
      || !result.sourceBlockIds.length || result.sourceBlockIds.length > 32
      || new Set(result.sourceBlockIds).size !== result.sourceBlockIds.length) continue;
    const indices = result.sourceBlockIds.map(id => ocr.blocks.findIndex(block => block.id === id));
    if (indices.some(index => index < 0)) continue;
    const blocks = indices.map(index => ocr.blocks[index]);
    // A forced rerun can reuse a frame ID. Check its source text as well.
    if (blocks.some(block => !result.sourceBlocks.some(source => source.id === block.id
      && normalize(source.text) === normalize(block.text)))) continue;
    const bbox = unionBounds(blocks, ocr.sourceWidth, ocr.sourceHeight);
    const id = indices.join(':');
    if (!bbox || seen.has(id)) continue;
    seen.add(id);
    items.push({id, text, bbox, sourceBlockIds: [...result.sourceBlockIds]});
    if (items.length === 64) break;
  }
  return items;
}

export function mergeTranslation(events: TranslationEvent[], next: TranslationEvent): TranslationEvent[] {
  const old = events.find(event => event.frameId === next.frameId && event.generation === next.generation);
  if (old && (old.epoch > next.epoch || (old.epoch === next.epoch && old.revision > next.revision))) return events;
  return [next, ...events.filter(event => event.frameId !== next.frameId || event.generation !== next.generation)].slice(0, 100);
}
