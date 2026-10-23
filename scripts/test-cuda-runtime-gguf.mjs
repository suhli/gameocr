// Real GGUF CUDA OCR using a locally built Worker and two downloaded NVIDIA components.
// The lean package no longer bundles this Worker; desktop-cuda-runtime.mjs covers its installation.
// GAMEOCR_CUDA_CACHE must point to an already installed runtime cache.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawn,execFileSync} from 'node:child_process';
import readline from 'node:readline';
const cache=path.resolve(process.env.GAMEOCR_CUDA_CACHE??'.cache/cuda-download-validation/data/runtimes/nvidia');
const root=path.resolve(process.env.GAMEOCR_WORKER_ROOT??'src-tauri/resources');
const catalog=JSON.parse(fs.readFileSync('src-tauri/catalog/cuda-runtime.json','utf8'));
const libraries=catalog.packages.filter(p=>p.runtimes.includes('llamacpp')).map(p=>path.join(cache,`${p.id}-${p.version}-${p.sha256.slice(0,12)}`));
for(const directory of libraries)assert(fs.existsSync(path.join(directory,'installed.json')));
const directory=path.join(root,'workers/llamacpp/cuda');
const env={...process.env};for(const key of Object.keys(env))if(key.toLowerCase()==='path'||key.startsWith('CUDA_PATH')||key==='ORT_DYLIB_PATH')delete env[key];
env.Path=[directory,...libraries,path.join(process.env.SystemRoot,'System32'),process.env.SystemRoot].join(';');
const child=spawn(path.join(directory,'ocr-worker.exe'),[],{cwd:directory,env,windowsHide:true,stdio:['pipe','pipe','pipe']});
let pending,stderr='';child.stderr.on('data',b=>stderr=(stderr+b).slice(-4096));
const exited=new Promise(resolve=>child.on('exit',resolve));
readline.createInterface({input:child.stdout}).on('line',line=>{if(pending){const p=pending;pending=null;clearTimeout(p.timer);p.resolve(JSON.parse(line));}});
child.on('error',e=>pending?.reject(e));child.on('exit',code=>{if(pending){clearTimeout(pending.timer);pending.reject(Error(`Exit ${code}: ${stderr}`));}});
const request=value=>new Promise((resolve,reject)=>{pending={resolve,reject,timer:setTimeout(()=>{child.kill();reject(Error('Inference timeout'));},180000)};child.stdin.write(JSON.stringify(value)+'\n');});
try{
  const manifest={schemaVersion:2,id:'Q8_0',name:'LightOnOCR-3 Q8_0',runtime:'llamacpp',adapter:'lightonocr',version:'3',quantization:'Q8_0',capabilities:{plain:true,grounding:true,nativeCoordinates:false,confidence:false,generation:true},files:[{role:'main',path:'LightOnOCR-3-0.8B-Q8_0.gguf',size:811843648,sha256:'b5c262bc905015e78002ea0e360bcbc7e6b3edd71d29a80857f677392f48eb15'},{role:'mmproj',path:'LightOnOCR-3-0.8B-mmproj-f16.gguf',size:204987136,sha256:'e77f2ebb738f5c7852e955f09440c407d9c54b56c6f6098900380706302e8742'}],source:'test',parameters:null,installed:true,imported:false};
  const ready=await request({type:'loadEngine',protocol_version:2,config:{manifest,directory:path.resolve('models'),device:'cuda',threads:4,inference:{runtime:'llamacpp',parameters:{model:'',mmproj:'',device:'cuda',threads:4,contextSize:8192,maxTokens:2048,temperature:0.1,maxImageEdge:1280}}}});
  assert.equal(ready.type,'ready',JSON.stringify(ready));
  const response=await request({type:'recognize',image:path.resolve('tests/fixtures/english.png'),frame_id:1,timestamp:Date.now(),mode:'grounding'});
  assert.equal(response.type,'result',JSON.stringify(response));assert.equal(response.result.device,'CUDA');assert(response.result.blocks.some(b=>b.text.includes('Health')));
  const modules=JSON.parse(execFileSync('powershell.exe',['-NoProfile','-Command',`ConvertTo-Json -Compress -InputObject @((Get-Process -Id ${child.pid}).Modules | Where-Object { $_.ModuleName -match '^(cudart|cublas)' } | Select-Object ModuleName,FileName)`],{windowsHide:true,encoding:'utf8'}));
  assert(modules.some(m=>m.ModuleName.toLowerCase().startsWith('cublas')));assert(modules.every(m=>path.resolve(m.FileName).toLowerCase().startsWith(cache.toLowerCase()+path.sep)));
  await request({type:'shutdown'});await exited;assert.equal(child.exitCode,0);
  fs.writeFileSync('docs/test-results/cuda-runtime-gguf.json',JSON.stringify({executedAt:new Date().toISOString(),libraries,modules,result:response.result},null,2)+'\n');
  console.log('Passed real GGUF CUDA OCR using the downloaded CUDA Runtime/cuBLAS libraries');
}finally{child.kill();}
