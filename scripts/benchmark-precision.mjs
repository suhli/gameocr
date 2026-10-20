// Evaluation, not a promotion gate: a failed/slower candidate stays experimental.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {engineConfig} from './executor-driver.mjs';
import {startNative} from './onnx-worker-driver.mjs';
import {accuracy, quantile, compareOutputs} from './ocr-quality.mjs';

const read=p=>JSON.parse(fs.readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const hash=p=>createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const corpus=read('tests/fixtures/small-text/corpus.json');
const profiles=[
  {name:'standard',parameters:{widthBuckets:true},scenes:[
    {name:'english',image:'tests/fixtures/english.png',tokens:['GAME OCR TEST','Health','Quest complete']},
    {name:'mixed-cjk',image:'tests/fixtures/mixed-cjk.png',tokens:['游戏','繁體中文','こんにちは','Health']},
    {name:'blank',image:'tests/fixtures/blank.png',groundTruth:[]},
    {...corpus.scenarios.find(s=>s.width===2560),image:'tests/fixtures/small-text/dense-2560.png'},
  ]},
  {name:'selective8',parameters:{widthBuckets:true,selectiveDetection:true,refinementRegions:8},
   scenes:[{...corpus.scenarios.find(s=>s.width===3840),image:'tests/fixtures/small-text/dense-3840.png'}]},
];
const variants=[
  {name:'fp32-cpu',device:'cpu'},
  {name:'int8-cpu',device:'cpu',mode:'int8'},
  {name:'fp32-cuda',device:'cuda'},
  {name:'fp16-cuda',device:'cuda',mode:'fp16'},
].filter(v=>!process.env.GAMEOCR_PRECISION_VARIANT||process.env.GAMEOCR_PRECISION_VARIANT.split(',').includes(v.name));
const report={executedAt:new Date().toISOString(),cpu:os.cpus()[0]?.model,
  scope:'Native Small adapter; 1 warmup + 3 measured runs; synthetic held-out images; no desktop capture/encoding',
  defaultChanged:false,models:[],results:[],comparisons:[]};
try{report.gpu=execFileSync('nvidia-smi.exe',['--query-gpu=name,driver_version,memory.total','--format=csv,noheader'],{encoding:'utf8',windowsHide:true}).trim();}catch{report.gpu=null;}
const destination=process.env.GAMEOCR_PRECISION_REPORT??'docs/test-results/precision.json';
const save=()=>fs.writeFileSync(destination,JSON.stringify(report,null,2)+'\n');
const baseline=new Map();
for(const variant of variants){
  const engine=engineConfig(variant.device);
  if(variant.mode){
    engine.directory=path.resolve(process.env.GAMEOCR_OPTIMIZED_MODELS??'.cache/optimized-models',`small-${variant.mode}`);
    engine.manifest=read(path.join(engine.directory,`ppocrv6-small-${variant.mode}-experimental.json`));
    // Calibration and evaluation images must not share content, even if renamed.
    const metadata=read(path.join(engine.directory,'conversion-metadata.json'));
    const training=new Set((metadata.calibration?.images??[]).map(i=>i.sha256));
    for(const profile of profiles)for(const scene of profile.scenes){
      if(training.has(hash(scene.image)))throw Error('Calibration/evaluation leakage: '+scene.image);
    }
  }
  report.models.push({name:variant.name,manifest:engine.manifest,
    modelBytes:engine.manifest.files.reduce((n,f)=>n+f.size,0)});
  for(const profile of profiles){
    const config=structuredClone(engine);Object.assign(config.inference.parameters,profile.parameters);
    const worker=await startNative(config);
    try{
      if(worker.ready.type!=='ready')throw Error(JSON.stringify(worker.ready));
      for(const scene of profile.scenes){
        const runs=[];
        for(let n=0;n<4;n++){
          const response=await worker.recognize(scene.image);
          if(response.type!=='result')throw Error(JSON.stringify(response));
          const result=response.result;
          const text=result.blocks.map(b=>b.text).join('\n');
          const quality=scene.groundTruth?accuracy(scene.groundTruth,result.blocks):{
            requiredTokensPresent:scene.tokens.every(t=>text.includes(t)),
          };
          if(!variant.mode&&!(quality.requiredTokensPresent??(
            quality.exactLineRecall===1&&quality.characterErrorRate===0&&quality.unexpectedBlocks===0))){
            throw Error('FP32 reference failed quality check: '+scene.name);
          }
          if(n)runs.push({result,quality});
        }
        const row={variant:variant.name,profile:profile.name,scene:scene.name,
          imageSha256:hash(scene.image),workerSha256:hash(worker.executable),
          loadMs:worker.ready.load_ms,quality:runs.map(r=>r.quality),
          p50Ms:quantile(runs.map(r=>r.result.metrics.totalMs),.5),
          p95Ms:quantile(runs.map(r=>r.result.metrics.totalMs),.95),
          detectionMs:quantile(runs.map(r=>r.result.metrics.detectionMs),.5),
          recognitionMs:quantile(runs.map(r=>r.result.metrics.recognitionMs),.5),
          runs:runs.map(r=>r.result.metrics),blocks:runs[0].result.blocks};
        const key=variant.device+'/'+scene.name;
        if(!variant.mode)baseline.set(key,row);
        else if(baseline.has(key)){
          const previous=baseline.get(key);
          report.comparisons.push({variant:variant.name,scene:scene.name,
            speedup:previous.p50Ms/row.p50Ms,
            outputComparison:compareOutputs(previous.blocks,row.blocks)});
        }
        report.results.push(row);save();
        console.log(JSON.stringify({variant:row.variant,scene:row.scene,p50Ms:row.p50Ms,quality:row.quality[0]}));
      }
    }catch(error){
      report.results.push({variant:variant.name,profile:profile.name,error:String(error)});save();
      console.error(variant.name,profile.name,String(error));
      if(!variant.mode)process.exitCode=1;
    }finally{await worker.close();}
  }
}
save();
