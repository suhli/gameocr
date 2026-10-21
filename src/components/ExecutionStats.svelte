<script lang="ts">
  import type {ExecutorStatus} from '../types';
  let {value}:{value?:ExecutorStatus|null}=$props();
  const ms=(n?:number|null)=>n==null?'—':`${n.toFixed(0)} ms`;
  const adaptive=$derived(value?.last?.adaptive);
  const paths:Record<string,string>={'full':'完整识别','cache-only':'复用文字','rec-only':'仅识别文字'};
  const reasons:Record<string,string>={'forced-full':'手动完整识别','periodic-refresh':'定期刷新','untrusted-boundaries':'正在确认文字边界','initial-or-invalidated':'首次识别或缓存已失效','exact-frame':'画面完全相同','trusted-boundaries':'文字边界稳定','new-edge-or-boundary-change':'新增文字或边界变化','fade-or-surrounding-change':'文字渐隐或周围变化','recognition-risk':'识别可信度不足','cache-only-content-changed':'文字区域发生变化','changed-empty-scene':'空白画面发生变化','frame-context':'来源或帧顺序变化','resize':'窗口尺寸变化'};
  const states:Record<string,string>={idle:'空闲',busy:'处理中',failed:'失败',stopped:'已停止'};
  const live=$derived(value?.workers.filter(w=>w.state!=='failed'&&w.state!=='stopped')??[]);
</script>
{#if value}
  <details class="execution-stats">
    <summary>{value.executionMode==='adaptive'?'自适应优化':value.parallel?'手动并行':'单 Worker'} · {value.last?.appLatency?.captureToPublishMs!=null?'采集至发布':'执行'} {ms(value.last?.appLatency?.captureToPublishMs??value.last?.endToEndMs)} · CPU {live.filter(w=>w.device==='CPU').length} / GPU {live.filter(w=>w.device==='CUDA').length} · 等待 {value.queuedTasks}</summary>
    {#if adaptive}<div class="timings" aria-label="自适应决策">
      <span>选择 {adaptive.strategy==='cpu-single'?'CPU×1':'CPU×2'}</span><span>特征 {ms(adaptive.analysisMs)} · 决策 {ms(adaptive.decisionMs)} · 切换/预热 {ms(adaptive.switchMs)}</span>
      <span>相似样本 {adaptive.similarSamples} · {adaptive.reliable?'证据充分':'学习数据不足'}</span><span>预计 {ms(adaptive.predictedMs)} · 实际 OCR {ms(adaptive.ocrMs)}</span>
      <span>历史中位 CPU×1 {ms(adaptive.singleMedianMs)} / CPU×2 {ms(adaptive.parallelMedianMs)}</span><span>{adaptive.exploring?'低频探索中':'未探索'} · 整图复验 {value.last?.wholeFrameFallback?'是':'否'}</span>
      <span>{adaptive.reason}</span>{#if adaptive.qualityRisk}<span>检测到完整性风险信号；置信度不能证明没有漏字</span>{/if}
    </div>{/if}
    {#if value.last}
      {#if value.last.temporal}<div class="timings" aria-label="时序识别统计"><span>路径 {paths[value.last.temporal.path]??value.last.temporal.path}</span><span>原因 {reasons[value.last.temporal.reason]??value.last.temporal.reason}</span><span>跟踪 {ms(value.last.temporal.trackerMs)}</span><span>复用 {value.last.temporal.reusedLines} 行 · 仅识别 {value.last.temporal.recOnlyRegions} 区域</span><span>完整检测 {value.last.temporal.fullDetections} 次 · 风险回退 {value.last.temporal.riskFallbacks} 次</span></div>{/if}
      {#if value.last.pipeline}{@const p=value.last.pipeline}<div class="timings" aria-label="识别阶段耗时"><span>读取图像 {ms(p.decodeMs)}</span><span>检测预处理 {ms(p.detectorPreprocessMs)} · DB {ms(p.dbMs)}</span><span>透视裁剪 {ms(p.cropMs)} · 识别预处理 {ms(p.recognizerPreprocessMs)}</span><span>CTC {ms(p.ctcMs)} · 缓存提交 {ms(p.cacheMs)}</span><span>实际检测 {p.detectorCalls} 次 · 识别 {p.recognizerCalls} 批</span><span>概率输出 {(p.outputBytes/1048576).toFixed(2)} MB</span>{#if p.compactStatus!=='disabled'}<span>概率压缩 {p.compactStatus} · 回退 {p.compactFallbacks}</span>{/if}</div>{/if}
      {#if value.last.appLatency}{@const a=value.last.appLatency}<div class="timings" aria-label="应用延迟"><span>采集读回 {ms(a.captureReadbackMs)} · RGB 转换 {ms(a.conversionMs)}</span><span>帧等待 {ms(a.frameWaitMs)} · 变化检查 {ms(a.gateMs)}</span><span>预览{a.previewReused?'复用':'编码'} {ms(a.previewMs)} · 发布 {ms(a.publishMs)}</span><span>采集回调至发布 {ms(a.captureToPublishMs)}（不含屏幕采样前等待及界面绘制）</span></div>{/if}
      <div class="timings"><span>传输方式 {value.last.transport??'文件'}</span><span>IPC 额外开销 {ms(value.last.ipcOverheadMs)}</span>{#if value.last.transportFallback}<span>传输回退：{value.last.transportFallback}</span>{/if}</div>
      {#if value.last.cache}<div class="timings" aria-label="OCR 缓存统计"><span>复用检测区域 {value.last.cache.reusedRegions} / {value.last.detectionRegions??0}</span><span>复用文字 {value.last.cache.reusedLines} 行</span><span>{value.last.cache.refreshed?'本帧刷新缓存':'逐像素校验缓存'}</span></div>{/if}
      <div class="timings"><span>传输 {ms(value.last.transferMs)}</span><span>切片 {ms(value.last.splitMs)}</span><span>最长排队 {ms(value.last.queueWaitMs)}</span><span>执行墙钟 {ms(value.last.workerWallMs)}</span><span>合并 {ms(value.last.mergeMs)}</span><span>任务 CPU {value.last.cpuTasks} / GPU {value.last.gpuTasks}</span><span>失败 {value.last.failedTasks} / 重试 {value.last.retriedTasks}</span>{#if value.last.wholeFrameFallback}<span>边界完整性：整图复验</span>{/if}{#if value.last.speedup!=null}<span>相对可比基线 {value.last.speedup.toFixed(2)}×</span>{/if}</div>
    {/if}
    <div class="workers">{#each value.workers as worker}<span>{worker.device} #{worker.id+1} · {states[worker.state]??worker.state} · {worker.threads} 线程 · {worker.completedTasks} 次 · 平均 {ms(worker.averageMs)}</span>{/each}</div>
  </details>
{/if}
<style>
  .execution-stats{padding:9px 22px;border-top:1px solid var(--border);background:var(--panel);font-size:10px;color:var(--muted);max-height:160px;overflow:auto;flex-shrink:0}
  summary{cursor:pointer;color:var(--foreground);line-height:1.6}.timings,.workers{display:flex;flex-wrap:wrap;gap:7px 14px;margin-top:10px;line-height:1.6}.workers span{display:block}
</style>
