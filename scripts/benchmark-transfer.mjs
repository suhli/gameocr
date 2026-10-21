import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {engineConfig,startExecutor,resources} from './executor-driver.mjs';import {compareOutputs,quantile} from './ocr-quality.mjs';
const report={executedAt:new Date().toISOString(),scope:'Same native adapter via desktop executor; full and repeated static-frame paths. Includes transfer/worker/merge, excludes WGC and JPEG/UI.',results:[]};
const baselines=new Map();const fourK=process.env.GAMEOCR_TRANSFER_4K==='1';const destination=process.env.GAMEOCR_TRANSFER_REPORT??'docs/test-results/shared-memory.json';
for(const device of ['cpu','cuda'])for(const temporal of (fourK?[true]:[false,true]))for(const shared of [false,true]){
 const engine=engineConfig(device);Object.assign(engine.inference.parameters,{preserveSmallText:fourK,experimentalSharedMemory:shared,temporal:{enabled:temporal}});
 const w=await startExecutor(`transfer-${device}-${temporal}-${shared}`,engine,{});
 try {
  assert.equal(w.ready.type,'ready',JSON.stringify(w.ready));
  for(const image of (fourK?['tests/fixtures/small-text/dense-3840.png']:['tests/fixtures/temporal/static-dialog-0.png','tests/fixtures/small-text/dense-2560.png'])){
   const runs=[];for(let i=0;i<5;i++){
    const response=await w.request({type:'recognize',image:path.resolve(image),frameId:i+1,generation:image.includes('dense')?2:1});assert.equal(response.type,'result',JSON.stringify(response));
    const r=response.result;assert.equal(r.metrics.execution.transport,shared?'shared-memory':'ppm');
    const key=device+temporal+image;if(!shared)baselines.set(key,r.blocks);else assert.equal(compareOutputs(baselines.get(key),r.blocks).disagreement,false);
    runs.push(r.metrics);
   }
   const warm=runs.slice(2);const row={device,temporal,shared,image,p50Ms:quantile(warm.map(r=>r.execution.endToEndMs),.5),transferMs:quantile(warm.map(r=>r.execution.transferMs),.5),decodeMs:quantile(warm.map(r=>r.pipeline.decodeMs),.5),resources:resources([w.child.pid]),runs};
   report.results.push(row);fs.writeFileSync(destination,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({device,temporal,shared,image,p50Ms:row.p50Ms,transferMs:row.transferMs,decodeMs:row.decodeMs}));
  }
  const cancel=await w.request({type:'cancel',image:path.resolve('tests/fixtures/small-text/dense-2560.png'),delayMs:2});assert.equal(cancel.type,'error');
  const recovered=await w.request({type:'recognize',image:path.resolve('tests/fixtures/english.png'),frameId:100,generation:10});assert.equal(recovered.type,'result');
 }finally{await w.close();}
}
