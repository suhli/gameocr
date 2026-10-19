// Explicit NVIDIA hardware regression for the Single and Manual manager paths.
import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {engineConfig,startExecutor} from './executor-driver.mjs';
const report={executedAt:new Date().toISOString(),binarySha256:createHash('sha256').update(fs.readFileSync(process.env.GAMEOCR_EXECUTOR_BENCH??'target/release/executor-bench.exe')).digest('hex'),runs:[]};
for(const executionMode of ['single','manual']){
  let driver;
  try {
    driver=await startExecutor('cuda-compat-'+executionMode,engineConfig('cuda'),{enableParallelInference:executionMode==='manual',gpuWorkerCount:2,cpuWorkerCount:0,enableCpuWorkers:false},undefined,{executionMode});
    assert.equal(driver.ready.type,'ready',JSON.stringify(driver.ready));assert.equal(driver.ready.device,'CUDA');
    assert.equal(driver.ready.executor.workers.length,executionMode==='manual'?2:1);
    assert(driver.ready.executor.workers.every(w=>w.device==='CUDA'));
    const result=await driver.request({type:'recognize',image:path.resolve('tests/fixtures/parallel/hud-1080.png'),frameId:1});
    assert.equal(result.type,'result',JSON.stringify(result));assert.equal(result.result.blocks.length,4);assert(result.result.blocks.some(b=>b.text.includes('Health')));
    assert.equal(result.result.metrics.execution.failedTasks,0);assert.equal(result.executor.executionMode,executionMode);
    report.runs.push({mode:executionMode,ready:driver.ready,result});
  }finally{await driver?.close();}
}
report.status='passed';fs.writeFileSync('docs/test-results/adaptive-cuda-executor.json',JSON.stringify(report,null,2)+'\n');console.log('Passed actual CUDA Single / Manual GPU×2 executor regression.');
