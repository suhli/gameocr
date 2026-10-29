import type {OcrResult, Preview} from '../../src/types';
import type {TranslationEvent, TranslationMetrics} from '../../src/types/translation';

export function previewFixture(): {frame: Preview; ocr: OcrResult; translation: TranslationEvent} {
  const frame = {frameId: 7, generation: 3, width: 1280, height: 720, dataUrl: ''};
  const ocr: OcrResult = {
    frameId: 7, timestamp: 1000, sourceWidth: 1280, sourceHeight: 720,
    blocks: [
      {id: 'a', text: 'New Game', bbox: [400, 200, 700, 260]},
      {id: 'b', text: 'Load Game', bbox: [400, 320, 700, 380]},
    ], rawOutput: '', metrics: {preprocessingMs: 0, inferenceMs: 0, parsingMs: 0, totalMs: 0},
    device: 'CPU', truncated: false, modelId: 'ppocrv6-small',
  };
  const identity = {frameId: 7, generation: 3, epoch: 2, revision: 5};
  const metrics: TranslationMetrics = {
    batchRequestId: null, batchSize: 1, modelLoadMs: 0, backendPrepareMs: 0, promptMs: 0,
    queueMs: 0, ttftMs: null, promptTokens: 0, generatedTokens: 0, tokensPerSecond: null,
    inferenceMs: 0, totalMs: 0, textProcessingMs: 0, publishMs: 0, ocrMs: 0,
    ocrToTranslationMs: 0, captureToTranslationMs: null, processMemoryMb: null, freeVramMb: null,
  };
  const translation: TranslationEvent = {...identity, results: ocr.blocks.map((block, index) => ({
    ...identity, profileId: '', requestId: String(index), sourceModelId: ocr.modelId!,
    translationModelId: 'hy-mt2', sourceLanguage: 'en', targetLanguage: 'zh',
    sourceText: block.text, translatedText: ['新游戏', '加载游戏'][index],
    sourceBlockIds: [block.id], sourceBlocks: [structuredClone(block)], glossaryVersion: 0,
    contextVersion: 0, cacheHit: false, status: 'complete', error: null,
    device: 'CPU', truncated: false, stopReason: 'eos', metrics,
  }))};
  return {frame, ocr, translation};
}
