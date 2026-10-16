import fs from 'node:fs';
import path from 'node:path';
import {spawn,execFileSync} from 'node:child_process';
import readline from 'node:readline';
export function engineConfig(device='cpu',batchSize=6,detectionResize=960,model='ppocrv6-small'){
  const directory=path.resolve('.cache/test-models');
  return {manifest:JSON.parse(fs.readFileSync(path.join(directory,model+'.json'),'utf8')),directory,device,threads:4,inference:{runtime:'onnx',parameters:{detectionThreshold:0.2,boxThreshold:0.45,unclipRatio:1.4,recognitionThreshold:0.5,batchSize,detectionResize}}};
}
export async function startExecutor(name,engine,parallel,resources=path.resolve('src-tauri/resources'),options={}){
  const root=path.resolve('.cache/executor-runs',name);fs.mkdirSync(root,{recursive:true});
  const config=path.join(root,'config.json');const cache=path.join(root,'frames');
  fs.writeFileSync(config,JSON.stringify({resources,engine,parallel,cache,...options}));
  const executable=path.resolve(process.env.GAMEOCR_EXECUTOR_BENCH??'target/release/executor-bench.exe');
  const child=spawn(executable,[config],{windowsHide:true,stdio:['pipe','pipe','pipe']});
  let waiting,stderr='',ended=false;const buffered=[];
  const exit=new Promise(resolve=>child.once('close',code=>{ended=true;if(waiting){clearTimeout(waiting.timer);waiting.reject(Error('Executor exited '+code+': '+stderr));waiting=null;}resolve(code);}));
  child.stderr.on('data',d=>stderr=(stderr+d).slice(-12000));
  child.on('error',e=>{if(waiting){clearTimeout(waiting.timer);waiting.reject(e);waiting=null;}});
  child.stdin.on('error',e=>{if(waiting){clearTimeout(waiting.timer);waiting.reject(e);waiting=null;}});
  readline.createInterface({input:child.stdout}).on('line',line=>{let value;try{value=JSON.parse(line);}catch(e){value={type:'error',message:String(e)+': '+line.slice(0,500)};}if(waiting){clearTimeout(waiting.timer);waiting.resolve(value);waiting=null;}else buffered.push(value);});
  function next(){if(buffered.length)return Promise.resolve(buffered.shift());if(ended)return Promise.reject(Error('Executor has exited: '+stderr));if(waiting)throw Error('Only one host request may be in flight');return new Promise((resolve,reject)=>{waiting={resolve,reject,timer:setTimeout(()=>{waiting=null;reject(Error('Executor timeout: '+stderr));},420000)};});}
  const request=value=>{const response=next();child.stdin.write(JSON.stringify(value)+'\n');return response;};
  async function close(){
    if(!ended&&ready.type==='ready'){try{await request({type:'shutdown'});}catch{}child.stdin.end();}
    let timer;
    const code=await Promise.race([exit,new Promise(r=>{timer=setTimeout(()=>r(null),5000);})]);
    clearTimeout(timer);
    if(code===null&&!ended){execFileSync('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});await exit;}
    if(fs.existsSync(cache)&&fs.readdirSync(cache).length)throw Error('Executor left frame temporary files: '+cache);
  }
  let ready;
  try{ready=await next();}catch(error){if(child.pid&&!ended){try{execFileSync('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});}catch{}}throw error;}
  return {child,ready,request,close,cache};
}
export function resources(pids){
  let processes=null,gpu=null;
  try{processes=JSON.parse(execFileSync('powershell.exe',['-NoProfile','-Command',`Get-Process -Id @(${pids.filter(Number.isInteger).join(',')}) -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,CPU,WorkingSet64,PrivateMemorySize64,PeakWorkingSet64 | ConvertTo-Json -Compress`],{windowsHide:true,encoding:'utf8',timeout:10000}));}catch{}
  try{gpu=execFileSync('nvidia-smi.exe',['--id=0','--query-gpu=name,driver_version,utilization.gpu,memory.used,memory.total','--format=csv,noheader,nounits'],{windowsHide:true,encoding:'utf8',timeout:3000}).trim();}catch{}
  return {at:new Date().toISOString(),processes,gpuDeviceWideSnapshot:gpu};
}
