<script lang="ts">
  import {api} from '../services/api';import type {TranslationEvent,TranslationStatus} from '../types/translation';
  let {entry,status,frameId,onError}:{entry:TranslationEvent|null;status:TranslationStatus|null;frameId:number;onError:(e:unknown)=>void}=$props();
  const ms=(n:number|null|undefined)=>n==null?'—':`${Math.round(n)} ms`;
  async function copy(){await navigator.clipboard.writeText(entry?.results.map(r=>r.translatedText).join('\n\n')??'');}
  async function exportJson(){const path=await api.saveJson();if(path)await api.exportTranslation(frameId,path);}
</script>
<section aria-label="翻译结果">
  <div class="row spread"><strong>译文</strong><div class="row"><button disabled={!status?.modelId} onclick={()=>api.translateOnce(frameId).catch(onError)}>翻译当前帧</button><button disabled={!entry} onclick={()=>copy().catch(onError)}>复制译文</button></div></div>
  {#if !entry}<p class="muted">{status?.status==='translating'?'正在翻译…':status?.status==='discarding'?'旧译文已取消，正在结束推理…':status?.modelId?'等待稳定文字或点击翻译当前帧':'请在翻译设置中加载模型'}</p>{/if}
  {#each entry?.results??[] as r}
    <article><p>{r.translatedText}</p>{#if r.error}<pre role="alert">{r.error}</pre>{/if}{#if r.truncated}<small>译文不完整 · {r.stopReason==='maxTokens'?'已达到输出上限':r.stopReason}</small>{/if}
      <small>{r.cacheHit?'缓存命中':r.device==='skipped'?'无需翻译':`${r.device} · 推理 ${ms(r.metrics.inferenceMs)} · ${r.metrics.tokensPerSecond?.toFixed(1)??'—'} token/s`}</small>
      {#if r.metrics.batchSize>1}<small>共 {r.metrics.batchSize} 组共享一次推理；耗时和 Token 仅在该批首项计入。</small>{/if}
      <details><summary>延迟详情</summary><small>OCR {ms(r.metrics.ocrMs)} · 文本处理 {ms(r.metrics.textProcessingMs)} · 排队 {ms(r.metrics.queueMs)}<br/>TTFT {ms(r.metrics.ttftMs)} · OCR→译文 {ms(r.metrics.ocrToTranslationMs)}<br/>截图→译文 {ms(r.metrics.captureToTranslationMs)} · Token {r.metrics.generatedTokens}<br/>Worker 内存 {r.metrics.processMemoryMb?.toFixed(0)??'—'} MiB</small></details>
    </article>
  {/each}
  {#if entry}<div class="row"><button onclick={()=>api.confirmTranslationContext(frameId).catch(onError)}>加入上下文</button><button onclick={()=>exportJson().catch(onError)}>导出译文 JSON</button></div>{/if}
  {#if status?.message}<pre role="status">{status.message}</pre>{/if}
  <small>模型调用 {status?.calls??0} · 缓存 {status?.cacheHits??0} · 丢弃 {status?.dropped??0}</small>
</section>
<style>
  section{padding:16px;border-top:1px solid var(--border)}strong{font-size:12px}button{background:var(--subtle);border:1px solid var(--border);color:var(--foreground);border-radius:4px;padding:5px 7px;font-size:10px}button:disabled{opacity:.45}article{border:1px solid var(--border);border-radius:6px;padding:12px;margin:12px 0}p{white-space:pre-wrap;overflow-wrap:anywhere;font-size:12px;line-height:1.8}small{display:block;font-size:10px;margin:8px 0;line-height:1.7}pre{font-size:10px;white-space:pre-wrap;overflow-wrap:anywhere;color:#e2818b;max-height:150px;overflow:auto}summary{font-size:10px;color:var(--muted);cursor:pointer}.row{flex-wrap:wrap}
</style>
