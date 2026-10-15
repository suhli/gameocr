<script lang="ts">
  import type {ExecutorStatus} from '../types';
  let {value}:{value?:ExecutorStatus|null}=$props();
  const ms=(n?:number)=>n==null?'—':`${n.toFixed(0)} ms`;
  const states:Record<string,string>={idle:'空闲',busy:'处理中',failed:'失败',stopped:'已停止'};
  const live=$derived(value?.workers.filter(w=>w.state!=='failed'&&w.state!=='stopped')??[]);
</script>
{#if value}
  <details class="execution-stats">
    <summary>{value.parallel?'并行':'单 Worker'} · 端到端 {ms(value.last?.endToEndMs)} · CPU {live.filter(w=>w.device==='CPU').length} / GPU {live.filter(w=>w.device==='CUDA').length} · 等待 {value.queuedTasks}</summary>
    {#if value.last}
      <div class="timings"><span>传输 {ms(value.last.transferMs)}</span><span>切片 {ms(value.last.splitMs)}</span><span>最长排队 {ms(value.last.queueWaitMs)}</span><span>执行墙钟 {ms(value.last.workerWallMs)}</span><span>合并 {ms(value.last.mergeMs)}</span><span>任务 CPU {value.last.cpuTasks} / GPU {value.last.gpuTasks}</span><span>失败 {value.last.failedTasks} / 重试 {value.last.retriedTasks}</span>{#if value.last.wholeFrameFallback}<span>边界完整性：整图复验</span>{/if}{#if value.last.speedup!=null}<span>相对可比基线 {value.last.speedup.toFixed(2)}×</span>{/if}</div>
    {/if}
    <div class="workers">{#each value.workers as worker}<span>{worker.device} #{worker.id+1} · {states[worker.state]??worker.state} · {worker.threads} 线程 · {worker.completedTasks} 次 · 平均 {ms(worker.averageMs)}</span>{/each}</div>
  </details>
{/if}
<style>
  .execution-stats{padding:9px 22px;border-top:1px solid var(--border);background:var(--panel);font-size:10px;color:var(--muted);max-height:160px;overflow:auto;flex-shrink:0}
  summary{cursor:pointer;color:var(--foreground);line-height:1.6}.timings,.workers{display:flex;flex-wrap:wrap;gap:7px 14px;margin-top:10px;line-height:1.6}.workers span{display:block}
</style>
