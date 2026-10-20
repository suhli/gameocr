import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {engineConfig,startExecutor} from './executor-driver.mjs';
import {accuracy,compareOutputs} from './ocr-quality.mjs';
const corpus=JSON.parse(fs.readFileSync('tests/fixtures/small-text/corpus.json','utf8').replace(/^\uFEFF/,''));
const gt=corpus.scenarios.find(s=>s.width===3840).groundTruth;
const frames=['dense-3840','dense-3840','dense-3840-changed','dense-3840-removed','dense-3840-scrolled'];
const report=[];const outputs=new Map();let id=0;
for(const incremental of [false,true]){
  const engine=engineConfig();Object.assign(engine.inference.parameters,{preserveSmallText:true,incremental});
  const host=await startExecutor('incremental-'+incremental,engine,{enableParallelInference:false});
  try{
    assert.equal(host.ready.type,'ready',JSON.stringify(host.ready));
    for(const [step,name] of frames.entries()){
      const response=await host.request({type:'recognize',image:path.resolve('tests/fixtures/small-text',name+'.png'),frameId:++id});
      assert.equal(response.type,'result',JSON.stringify(response));const r=response.result;
      const expected=structuredClone(gt);
      if(name.endsWith('-changed'))expected[0].text='Item 0-0 Health 79';
      if(name.endsWith('-removed'))expected.shift();
      if(name.endsWith('-scrolled'))for(const item of expected){item.bbox[1]+=27;item.bbox[3]+=27;}
      const quality=accuracy(expected,r.blocks);
      if(!incremental)outputs.set(step,r.blocks);
      else {
        assert.equal(compareOutputs(outputs.get(step),r.blocks).disagreement,false,'Cached output differs from uncached');
        if(step===0)assert(r.metrics.cache.refreshed);
        if(step===1){assert.equal(r.metrics.cache.reusedRegions,19);assert.equal(r.metrics.cache.reusedLines,53);assert.equal(r.metrics.inferenceMs,0);}
        if(step===2){assert(r.metrics.cache.reusedRegions>0&&r.metrics.cache.reusedRegions<19);assert(r.metrics.cache.reusedLines>0&&r.metrics.cache.reusedLines<53);}
      }
      const row={incremental,step,name,quality,metrics:r.metrics};report.push(row);console.log(JSON.stringify(row));
    }
    if(incremental){
      const image=path.resolve('tests/fixtures/small-text/dense-3840-scrolled.png');
      const changed=await host.request({type:'recognize',image,frameId:++id,generation:2});
      assert.equal(changed.type,'result');assert.equal(changed.result.metrics.cache.reusedRegions,0);assert(changed.result.metrics.cache.refreshed);
      report.push({check:'source-generation-invalidates',metrics:changed.result.metrics});
      const cancelled=await host.request({type:'cancel',image,delayMs:30});assert.equal(cancelled.type,'error');
      const after=await host.request({type:'recognize',image,frameId:++id});assert.equal(after.type,'result');assert(after.result.metrics.cache.refreshed);
      report.push({check:'cancel-invalidates',metrics:after.result.metrics});
    }
  }finally{await host.close();}
}
fs.writeFileSync('docs/test-results/incremental.json',JSON.stringify({executedAt:new Date().toISOString(),report},null,2)+'\n');
console.log('Incremental exactness, small-number changes, deletion, scroll, source switch and cancellation passed.');
