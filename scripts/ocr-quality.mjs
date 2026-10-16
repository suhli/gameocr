// Labels are for evaluation only; neither the executor nor scheduler sees them.
export const quantile=(values,p)=>values.length?[...values].sort((a,b)=>a-b)[Math.max(0,Math.ceil(values.length*p)-1)]:null;
export const normalize=s=>s.normalize('NFKC').replace(/\s+/gu,'').toLowerCase();
function distance(a,b){a=[...a];b=[...b];let prev=Array.from({length:b.length+1},(_,i)=>i);for(let i=0;i<a.length;i++){const next=[i+1];for(let j=0;j<b.length;j++)next.push(Math.min(prev[j]+(a[i]===b[j]?0:1),prev[j+1]+1,next[j]+1));prev=next;}return prev[b.length];}
export function iou(a,b){const inter=Math.max(0,Math.min(a[2],b[2])-Math.max(a[0],b[0]))*Math.max(0,Math.min(a[3],b[3])-Math.max(a[1],b[1]));const area=r=>Math.max(0,r[2]-r[0])*Math.max(0,r[3]-r[1]);return inter/Math.max(1,area(a)+area(b)-inter);}
export function accuracy(expected,blocks){
  const used=new Set();let exact=0,matched=0,errors=0,characters=0,overlap=0,center=0;
  for(const gt of expected){const candidates=blocks.map((b,i)=>({b,i,overlap:b.bbox?iou(gt.bbox,b.bbox):0})).filter(v=>!used.has(v.i)).sort((a,b)=>b.overlap-a.overlap);const best=candidates[0]?.overlap>0.05?candidates[0]:null;const text=normalize(gt.text);characters+=[...text].length;
    if(best){used.add(best.i);matched++;const recognized=normalize(best.b.text);errors+=distance(text,recognized);if(text===recognized)exact++;overlap+=best.overlap;const a=gt.bbox,b=best.b.bbox;center+=Math.hypot((a[0]+a[2]-b[0]-b[2])/2,(a[1]+a[3]-b[1]-b[3])/2);}else errors+=[...text].length;
  }
  for(const [i,b] of blocks.entries())if(!used.has(i))errors+=[...normalize(b.text)].length;
  return {exactLineRecall:expected.length?exact/expected.length:Number(blocks.length===0),characterErrorRate:errors/Math.max(1,characters),meanBoxIoU:matched?overlap/matched:null,meanCenterErrorPx:matched?center/matched:null,unexpectedBlocks:blocks.length-used.size};
}
export function compareOutputs(a,b){
  const missing=a.filter(x=>!b.some(y=>normalize(x.text)===normalize(y.text)&&x.bbox&&y.bbox&&iou(x.bbox,y.bbox)>0.5));
  const extra=b.filter(x=>!a.some(y=>normalize(x.text)===normalize(y.text)&&x.bbox&&y.bbox&&iou(x.bbox,y.bbox)>0.5));
  return {firstBlocks:a.length,secondBlocks:b.length,missingFromSecond:missing.length,extraInSecond:extra.length,disagreement:missing.length>0||extra.length>0,interpretation:'Output disagreement is uncertainty, not evidence that either strategy is correct.'};
}
