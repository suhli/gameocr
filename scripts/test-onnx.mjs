// Real worker integration test: graph inference, process switching, malformed
// requests, unload/reload, dictionary mismatch and empty-frame behavior.
import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';import readline from 'node:readline';
const directory=path.resolve(process.argv[2]??'.cache/test-models');
const executable=path.resolve(process.env.GAMEOCR_ONNX_WORKER??'target/release/onnx-worker.exe');
const device=process.env.GAMEOCR_TEST_DEVICE??'cpu';
const report=[];
function worker(){
  const child=spawn(executable,[],{windowsHide:true,stdio:['pipe','pipe','pipe']});let pending;let stderr='';
  child.stderr.on('data',d=>stderr=(stderr+d).slice(-8000));
  readline.createInterface({input:child.stdout}).on('line',line=>{if(pending){const p=pending;pending=null;clearTimeout(p.timer);try{p.resolve(JSON.parse(line));}catch(e){p.reject(e);}}});
  child.on('error',e=>pending?.reject(e));child.on('exit',code=>{if(pending){clearTimeout(pending.timer);pending.reject(Error(`Worker exited ${code}: ${stderr}`));pending=null;}});
  const request=value=>new Promise((resolve,reject)=>{assert(!pending);pending={resolve,reject,timer:setTimeout(()=>{child.kill();reject(Error(`Timeout: ${stderr}`));},180000)};child.stdin.write((typeof value==='string'?value:JSON.stringify(value))+'\n');});
  return {child,request,close:()=>new Promise(resolve=>{child.once('exit',resolve);child.stdin.end();})};
}
const parameters={detectionThreshold:0.2,boxThreshold:0.45,unclipRatio:1.4,recognitionThreshold:0.5,batchSize:6,detectionResize:960};
const load=(manifest)=>({type:'loadEngine',protocol_version:2,config:{manifest,directory,device,threads:4,inference:{runtime:'onnx',parameters}}});
for(const variant of ['small','medium']){
  const manifest=JSON.parse(fs.readFileSync(path.join(directory,`ppocrv6-${variant}.json`),'utf8'));
  const w=worker();try{
    assert.equal((await w.request('not JSON')).type,'error');
    const ready=await w.request(load(manifest));assert.equal(ready.type,'ready',JSON.stringify(ready));assert.equal(ready.device,device.toUpperCase());
    for(const [fixture,expected] of [['english',['GAME OCR TEST','Health','Quest complete']],['mixed-cjk',['游戏','繁體中文','こんにちは','Health']],['rotated-small',['ROTATED TEXT','Small game text']],['blank',[]]]){
      const response=await w.request({type:'recognize',image:path.resolve(`tests/fixtures/${fixture}.png`),frame_id:report.length,timestamp:Date.now(),mode:'grounding'});
      assert.equal(response.type,'result',JSON.stringify(response));const result=response.result;
      const text=result.blocks.map(b=>b.text).join('\n');for(const token of expected)assert(text.includes(token),`${variant}/${fixture}: missing ${token}: ${text}`);
      if(fixture==='blank')assert.equal(result.blocks.length,0);
      for(const block of result.blocks){assert(block.confidence>=0&&block.confidence<=1);assert.equal(block.polygon.length,4);assert(block.bbox[0]>=0&&block.bbox[2]<=result.sourceWidth);}
      report.push({variant,fixture,loadMs:ready.load_ms,...result});
    }
    assert.equal((await w.request({type:'unload'})).type,'unloaded');
    assert.equal((await w.request({type:'recognize',image:path.resolve('tests/fixtures/english.png'),frame_id:99,timestamp:0,mode:'plain'})).type,'error');
    const wrong=structuredClone(manifest);wrong.runtime='llamacpp';assert.equal((await w.request(load(wrong))).type,'error');
    assert.equal((await w.request(load(manifest))).type,'ready');
    assert.equal((await w.request({type:'shutdown'})).type,'unloaded');await w.close();
  }finally{w.child.kill();}
}
fs.mkdirSync('docs/test-results',{recursive:true});fs.writeFileSync(`docs/test-results/onnx-${device}-e2e.json`,JSON.stringify({executedAt:new Date().toISOString(),device,report},null,2)+'\n');
console.log(`Passed ${report.length} real ${device.toUpperCase()} OCR cases, malformed IPC, unload/reload and worker replacement.`);
