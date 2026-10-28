// Runs real native Hy-MT2 inference. Execute after builds finish, without other benchmarks.
import fs from 'node:fs';
import path from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
import readline from 'node:readline';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';

const model=path.resolve(process.env.GAMEOCR_TRANSLATION_MODEL??'.cache/translation/Hy-MT2-1.8B-Q4_K_M.gguf');
const out=path.resolve(process.env.GAMEOCR_TRANSLATION_REPORT_DIR??'docs/test-results');
fs.mkdirSync(out,{recursive:true});fs.mkdirSync('.cache/translation',{recursive:true});
const env={...process.env};
const pathKey=Object.keys(env).find(k=>k.toLowerCase()==='path')??'Path';
env[pathKey]=path.resolve('src-tauri/resources/workers/cuda-runtime')+';'+env[pathKey];
function run(name,device,input,extra=[]) {
  const binary=path.resolve(`target/workers-${device}/release/translation-test.exe`),output=path.join(out,`translation-${name}.json`);
  const log=fs.openSync(`.cache/translation/${name}-final.log`,'w');
  try {
    const execution=spawnSync(binary,['--model',model,'--device',device,'--threads','4','--input',input,'--output',output,...extra],{env,windowsHide:true,stdio:['ignore',log,log],timeout:300000});
    assert.equal(execution.status,0,`${name}: ${execution.error??'see native log'}`);
  } finally {fs.closeSync(log);}
  const report=JSON.parse(fs.readFileSync(output,'utf8'));
  report.executedAt=new Date().toISOString();
  report.testBinarySha256=createHash('sha256').update(fs.readFileSync(binary)).digest('hex');
  report.scope='Real model, synthetic game text; 4 CPU threads; no cache; one sequential pass, no other GameOCR build or benchmark running.';
  assert(report.results.every(r=>r.result.device===device));
  if(!extra.includes('--max-tokens'))assert(report.results.every(r=>r.result.stopReason==='eos'&&!r.result.truncated));
  if(input.endsWith('corpus.json'))assert.equal(report.results[0].result.text,report.results.at(-1).result.text,'KV isolation');
  fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n');console.log('PASS',name,report.results.length,'real translations');return report;
}
run('cpu','cpu','tests/translation/corpus.json');
run('cpu-low-temperature','cpu','tests/translation/corpus.json',['--temperature','0.2']);
if(process.env.GAMEOCR_SKIP_CUDA!=='1')run('cuda','cuda','tests/translation/corpus.json');
const batch=run('batch','cpu','tests/translation/batch.json');
const items=JSON.parse(batch.results[0].result.text);
assert.equal(items.length,3);assert.deepEqual(items.map(i=>i.id).sort(),['0','1','2']);assert(items.every(i=>i.text));
const limited=run('output-limit','cpu','tests/translation/corpus.json',['--max-tokens','1']);
assert(limited.results.every(r=>r.result.truncated&&r.result.stopReason==='maxTokens'&&r.result.metrics.generatedTokens===1));

// Real worker IPC cancellation, separate from deterministic fake-engine unit tests.
const worker=path.resolve('target/workers-cpu/release/translation-worker.exe');
const child=spawn(worker,[],{env,windowsHide:true,stdio:['pipe','pipe','pipe']});
let pending,stderr='';child.stderr.on('data',b=>stderr=(stderr+b).slice(-4096));
const exited=new Promise(resolve=>child.once('exit',resolve));
readline.createInterface({input:child.stdout}).on('line',line=>{if(pending){const p=pending;pending=null;clearTimeout(p.timer);p.resolve(JSON.parse(line));}});
child.on('error',e=>pending?.reject(e));child.on('exit',code=>{if(pending){clearTimeout(pending.timer);pending.reject(Error(`Worker exited ${code}: ${stderr}`));}});
const send=value=>child.stdin.write(JSON.stringify(value)+'\n');
const request=value=>new Promise((resolve,reject)=>{pending={resolve,reject,timer:setTimeout(()=>{child.kill();reject(Error('Worker timeout'));},180000)};send(value);});
try {
  const manifest=JSON.parse(fs.readFileSync(path.join(out,'translation-cpu.json'),'utf8')).model;
  const ready=await request({type:'load',protocol_version:1,config:{manifest,path:model,device:'cpu',threads:4,generation:{contextSize:4096,maxTokens:1024,temperature:0,topP:0.6,topK:20,repetitionPenalty:1.05,seed:42}}});
  assert.equal(ready.type,'ready',JSON.stringify(ready));assert.equal(ready.supports_cancellation,true);
  const pid=child.pid,started=performance.now();
  const running=request({type:'translate',request:{requestId:'obsolete',sourceText:('Before you leave for the fortress, speak to Captain Rowan and bring the sealed letter. ').repeat(30),options:{sourceLanguage:'en',targetLanguage:'zh'}}});
  const timer=setTimeout(()=>send({type:'cancel',request_id:'obsolete'}),150);
  const cancelled=await running;clearTimeout(timer);
  assert.equal(cancelled.type,'result',JSON.stringify(cancelled));assert.equal(cancelled.result.stopReason,'cancelled');
  const elapsedMs=performance.now()-started;
  const next=await request({type:'translate',request:{requestId:'latest',sourceText:'Where are you going?',options:{sourceLanguage:'en',targetLanguage:'zh'}}});
  assert.equal(next.type,'result',JSON.stringify(next));assert.equal(next.result.stopReason,'eos');assert(next.result.text);assert.equal(child.pid,pid);
  fs.writeFileSync(path.join(out,'translation-cancellation.json'),JSON.stringify({executedAt:new Date().toISOString(),scope:'Real CPU Hy-MT2 native worker; cancellation between decode calls; same process/model reused',workerSha256:createHash('sha256').update(fs.readFileSync(worker)).digest('hex'),elapsedMs,cancelled:cancelled.result,next:next.result},null,2)+'\n');
  await request({type:'shutdown'});await exited;assert.equal(child.exitCode,0);
  console.log('PASS real running cancellation and subsequent translation with same loaded worker');
} finally {if(child.exitCode===null)child.kill();}
