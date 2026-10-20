import fs from 'node:fs';import assert from 'node:assert/strict';
import {engineConfig} from './executor-driver.mjs';import {startNative} from './onnx-worker-driver.mjs';import {accuracy,quantile} from './ocr-quality.mjs';
const corpus=JSON.parse(fs.readFileSync('tests/fixtures/small-text/corpus.json','utf8').replace(/^\uFEFF/,''));const report=[];
for(const [name,parameters] of Object.entries({whole:{},selective4:{selectiveDetection:true,refinementRegions:4},selective8:{selectiveDetection:true,refinementRegions:8},full:{preserveSmallText:true}})){
  const engine=engineConfig();Object.assign(engine.inference.parameters,parameters);const worker=await startNative(engine);
  try{
    assert.equal(worker.ready.type,'ready',JSON.stringify(worker.ready));
    for(const scene of corpus.scenarios){
      const runs=[];for(let n=0;n<4;n++){const r=await worker.recognize('tests/fixtures/small-text/'+scene.image);assert.equal(r.type,'result',JSON.stringify(r));if(n)runs.push(r.result);}
      const row={name,scene:scene.name,quality:accuracy(scene.groundTruth,runs[0].blocks),p50Ms:quantile(runs.map(r=>r.metrics.totalMs),.5),regions:runs[0].metrics.detectionRegions};report.push(row);console.log(JSON.stringify(row));
      if(parameters.selectiveDetection)assert(row.regions<=parameters.refinementRegions+1);
    }
    const blank=await worker.recognize('tests/fixtures/blank.png');assert.equal(blank.type,'result');assert.equal(blank.result.blocks.length,0);
  }finally{await worker.close();}
}
fs.writeFileSync('docs/test-results/refinement.json',JSON.stringify({executedAt:new Date().toISOString(),report},null,2)+'\n');
const large=report.filter(r=>r.scene==='dense-3840');assert(large.find(r=>r.name==='selective4').quality.exactLineRecall>large.find(r=>r.name==='whole').quality.exactLineRecall);
for(const scene of corpus.scenarios){assert.equal(report.find(r=>r.scene===scene.name&&r.name==='full').quality.exactLineRecall,1);}
