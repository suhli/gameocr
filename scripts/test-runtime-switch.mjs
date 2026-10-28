// Exercises the same v2 contracts as the Tauri WorkerManager with real models.
import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';import {spawn} from 'node:child_process';import readline from 'node:readline';
const root=path.resolve('src-tauri/resources/workers');const reports=[];
async function run(runtime,manifest,directory,inference){
  const worker=path.join(root,runtime,'cpu',runtime==='onnx'?'onnx-worker.exe':'ocr-worker.exe');
  const child=spawn(worker,[],{cwd:path.dirname(worker),windowsHide:true,stdio:['pipe','pipe','pipe']});
  let pending,stderr='';child.stderr.on('data',b=>stderr=(stderr+b).slice(-4096));
  const exited=new Promise(resolve=>child.on('exit',resolve));
  readline.createInterface({input:child.stdout}).on('line',line=>{if(pending){const p=pending;pending=null;clearTimeout(p.timer);p.resolve(JSON.parse(line));}});
  child.on('error',e=>pending?.reject(e));child.on('exit',code=>{if(pending){clearTimeout(pending.timer);pending.reject(Error(`Exit ${code}: ${stderr}`));}});
  const request=value=>new Promise((resolve,reject)=>{pending={resolve,reject,timer:setTimeout(()=>{child.kill();reject(Error('Inference timeout'));},180000)};child.stdin.write(JSON.stringify(value)+'\n');});
  try{
    const ready=await request({type:'loadEngine',protocol_version:2,config:{manifest,directory,device:'cpu',threads:4,inference}});assert.equal(ready.type,'ready',JSON.stringify(ready));
    const response=await request({type:'recognize',image:path.resolve('tests/fixtures/english.png'),frame_id:reports.length,timestamp:Date.now(),mode:'grounding'});assert.equal(response.type,'result',JSON.stringify(response));
    assert.equal(response.result.runtime,runtime);assert.equal(response.result.modelId,manifest.id);assert(response.result.blocks.some(b=>b.text.includes('Health')));assert(response.result.blocks.some(b=>b.bbox));
    if(runtime==='llamacpp')assert(response.result.blocks.every(b=>b.confidence===null));
    reports.push(response.result);await request({type:'shutdown'});await exited;assert.equal(child.exitCode,0);
  }finally{child.kill();}
}
const catalog=JSON.parse(fs.readFileSync('src-tauri/catalog/ppocrv6.json','utf8'));const directory=path.resolve('.cache/test-models');
const onnx={runtime:'onnx',parameters:{detectionThreshold:0.2,boxThreshold:0.45,unclipRatio:1.4,recognitionThreshold:0.5,batchSize:6,detectionResize:960}};
const gguf={schemaVersion:2,id:'Q8_0',name:'LightOnOCR-3 Q8_0',runtime:'llamacpp',adapter:'lightonocr',version:'3',quantization:'Q8_0',capabilities:{plain:true,grounding:true,nativeCoordinates:false,confidence:false,generation:true},files:[{role:'main',path:'LightOnOCR-3-0.8B-Q8_0.gguf',size:811843648,sha256:'b5c262bc905015e78002ea0e360bcbc7e6b3edd71d29a80857f677392f48eb15'},{role:'mmproj',path:'LightOnOCR-3-0.8B-mmproj-f16.gguf',size:204987136,sha256:'e77f2ebb738f5c7852e955f09440c407d9c54b56c6f6098900380706302e8742'}],source:'test',parameters:null,installed:true,imported:false};
await run('onnx',catalog[0],directory,onnx);
await run('llamacpp',gguf,path.resolve('models'),{runtime:'llamacpp',parameters:{model:'',mmproj:'',device:'cpu',threads:4,contextSize:8192,maxTokens:2048,temperature:0.1,maxImageEdge:1280}});
await run('onnx',catalog[1],directory,onnx);
fs.writeFileSync(process.env.GAMEOCR_RUNTIME_SWITCH_REPORT??'docs/test-results/runtime-switch.json',JSON.stringify(reports,null,2)+'\n');
console.log('Passed actual ONNX small → GGUF Q8_0 → ONNX medium worker lifecycle and identity checks.');
