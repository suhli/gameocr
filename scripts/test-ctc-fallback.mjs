import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import assert from 'node:assert/strict';
import {engineConfig} from './executor-driver.mjs';import {startNative} from './onnx-worker-driver.mjs';import {compareOutputs} from './ocr-quality.mjs';
const checks=[];
for(const device of ['cpu','cuda']){
 let baseline;
 for(const mode of ['off','missing-compact','invalid-compact']){
  const engine=engineConfig(device);engine.inference.parameters.experimentalGpuCtc=mode!=='off';
  if(mode==='invalid-compact'){
   const directory=path.resolve('.cache/ctc-fallback');fs.mkdirSync(directory,{recursive:true});const bytes=Buffer.from('deliberately invalid ONNX for fallback testing');fs.writeFileSync(path.join(directory,'invalid.onnx'),bytes);
   // Keep actual verified detector/recognizer weights; only the optional graph is malformed.
   for(const f of engine.manifest.files){const source=path.join(engine.directory,f.path),target=path.join(directory,f.path);fs.mkdirSync(path.dirname(target),{recursive:true});if(!fs.existsSync(target))fs.copyFileSync(source,target);}
   engine.directory=directory;engine.manifest.files.push({role:'recognizer-ctc',path:'invalid.onnx',size:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex')});
  }
  const w=await startNative(engine);try{
   assert.equal(w.ready.type,'ready',JSON.stringify(w.ready));const r=await w.recognize('tests/fixtures/english.png');assert.equal(r.type,'result',JSON.stringify(r));
   if(mode==='off')baseline=r.result.blocks;else assert.equal(compareOutputs(baseline,r.result.blocks).disagreement,false);
   assert.equal(r.result.metrics.pipeline.compactBatches,0);checks.push({device,mode,status:r.result.metrics.pipeline.compactStatus,originalRecognizerRan:r.result.metrics.pipeline.recognizerCalls>0});
  }finally{await w.close();}
 }
}
fs.writeFileSync('docs/test-results/compact-ctc-fallback.json',JSON.stringify({executedAt:new Date().toISOString(),checks},null,2)+'\n');
