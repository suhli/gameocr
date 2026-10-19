import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {engineConfig,startExecutor} from './executor-driver.mjs';
import {accuracy,normalize,quantile} from './ocr-quality.mjs';
const corpus=JSON.parse(fs.readFileSync('tests/fixtures/small-text/corpus.json','utf8').replace(/^\uFEFF/,''));
const report=[];let id=0;
for(const [name,parameters] of Object.entries({whole:{},tiles:{preserveSmallText:true},roi:{detectionRegion:[.5,.7,.5,.3]},roiTiles:{detectionRegion:[.5,.7,.5,.3],preserveSmallText:true}})){
  const engine=engineConfig();Object.assign(engine.inference.parameters,parameters);
  const host=await startExecutor('small-text-'+name,engine,{enableParallelInference:name==='roiTiles',cpuWorkerCount:2,gpuWorkerCount:0});
  try{
    assert.equal(host.ready.type,'ready',JSON.stringify(host.ready));
    for(const scene of corpus.scenarios){
      const runs=[];
      for(let n=0;n<4;n++){
        const r=await host.request({type:'recognize',image:path.resolve('tests/fixtures/small-text',scene.image),frameId:++id});
        assert.equal(r.type,'result',JSON.stringify(r));const result=r.result;
        assert.equal(result.sourceWidth,scene.width);assert.equal(result.sourceHeight,scene.height);
        for(const b of result.blocks){assert(b.bbox[0]>=0&&b.bbox[2]<=scene.width&&b.bbox[1]>=0&&b.bbox[3]<=scene.height);}
        if(n)runs.push(result);
      }
      const expected=parameters.detectionRegion?scene.groundTruth.filter(b=>b.bbox[0]>=scene.width*.5&&b.bbox[1]>=scene.height*.7):scene.groundTruth;
      const result=runs[0];const quality=accuracy(expected,result.blocks);
      if(name==='tiles')assert(result.blocks.some(b=>normalize(b.text).includes('crosstileboundaryeverycharacter')),'Seam sentence missing');
      if(name.startsWith('roi'))assert(result.blocks.every(b=>b.bbox[0]>=scene.width*.5&&b.bbox[1]>=scene.height*.7),'ROI leaked');
      const row={name,scene:scene.name,quality,p50Ms:quantile(runs.map(r=>r.metrics.execution.endToEndMs),.5),regions:result.metrics.detectionRegions,result};
      report.push(row);console.log(JSON.stringify({...row,result:undefined}));
    }
  }finally{await host.close();}
}
for(const scene of corpus.scenarios){
  const baseline=report.find(r=>r.name==='whole'&&r.scene===scene.name),tiles=report.find(r=>r.name==='tiles'&&r.scene===scene.name);
  assert(tiles.quality.exactLineRecall>baseline.quality.exactLineRecall,'Expected small-text recall improvement');
  assert(tiles.quality.exactLineRecall>=.9,'Small-text recall below acceptance target');
  assert.equal(tiles.quality.unexpectedBlocks,0,'Duplicate/extra blocks');
}
fs.writeFileSync('docs/test-results/small-text.json',JSON.stringify({executedAt:new Date().toISOString(),report},null,2)+'\n');
