import {normalize} from './ocr-quality.mjs';

// Frames are processed sequentially, not replayed on a real-time capture clock.
// Stop when the annotation changes again; an unrelated later result is not recovery.
export function transitions(frames, runs) {
  const truth = frames.map(f => new Set(f.groundTruth.map(g => normalize(g.text))));
  const visible = runs.map(r => new Set(r.blocks.map(b => normalize(b.text))));
  const rows = [];
  for (let i = 1; i < frames.length; i++) {
    const added = [...truth[i]].filter(t => !truth[i - 1].has(t));
    const removed = [...truth[i - 1]].filter(t => !truth[i].has(t));
    if (!added.length && !removed.length) continue;
    function recovery(texts, present) {
      if (!texts.length) return {frames: 0, sequentialResponseMs: 0};
      let elapsed = 0;
      for (let j = i; j < runs.length; j++) {
        if (!texts.every(t => truth[j].has(t) === present)) break;
        elapsed += runs[j].requestMs;
        if (texts.every(t => visible[j].has(t) === present)) {
          return {frames: j - i, sequentialResponseMs: elapsed};
        }
      }
      return {frames: null, sequentialResponseMs: null};
    }
    const fresh = recovery(added, true), cleared = recovery(removed, false);
    rows.push({frame: i, added, removed,
      missingNew: added.filter(t => !visible[i].has(t)),
      staleOld: removed.filter(t => visible[i].has(t)),
      firstResponseMs: runs[i].requestMs,
      newTextDelayFrames: fresh.frames, staleDurationFrames: cleared.frames,
      newTextSequentialResponseMs: fresh.sequentialResponseMs,
      staleClearSequentialResponseMs: cleared.sequentialResponseMs});
  }
  return rows;
}
