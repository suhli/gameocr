// Runs real native Hy-MT2 inference. Execute after builds finish, without other benchmarks.
import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
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
