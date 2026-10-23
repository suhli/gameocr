<script lang="ts">
  import {onMount} from 'svelte';
  import {Button} from '$lib/components/ui/button';
  import {api,Subscriptions} from '../services/api';
  import type {TranslationModel,TranslationStatus} from '../types/translation';
  import type {DownloadProgress} from '../types';
  let models=$state<TranslationModel[]>([]),status=$state<TranslationStatus|null>(null),busy=$state(''),error=$state(''),progress=$state<DownloadProgress|null>(null);
  async function refresh(){[models,status]=await Promise.all([api.translationModels(),api.translationStatus()]);}
  async function run(name:string,fn:()=>Promise<unknown>){busy=name;error='';try{await fn();await refresh();}catch(e){error=String(e);}finally{busy='';}}
  async function local(){const path=await api.pickModel('导入官方 Hy-MT2-1.8B Q4_K_M');if(typeof path==='string')await run('import',()=>api.importTranslation(path));}
  onMount(()=>{const events=new Subscriptions();void refresh().catch(e=>error=String(e));void events.add('translation-status',s=>status=s);void events.add('translation-download-progress',p=>progress=p);return()=>events.dispose();});
</script>
<section class="translation-models">
  <h3>翻译模型</h3><p>翻译与 OCR 独立加载。模型按需下载，加载前核验 SHA-256。</p>
  {#each models as model}
    <article><strong>{model.name}</strong><p>{(model.size/1024**3).toFixed(2)} GiB · {model.license} · {model.runtime}</p>
      <p>当前状态：{status?.status??'unloaded'} · {status?.device??'尚未加载'}</p>
      <div class="row">
        {#if model.installed}<Button size="sm" disabled={!!busy} onclick={()=>run('load',()=>api.loadTranslation(model.id))}>加载翻译模型</Button><Button size="sm" variant="outline" disabled={!!busy||!status?.modelId} onclick={()=>run('unload',api.unloadTranslation)}>卸载</Button><Button size="sm" variant="outline" disabled={!!busy||status?.modelId===model.id} onclick={async()=>{if(await api.confirmDelete(model.name))await run('delete',()=>api.deleteTranslation(model.id));}}>删除</Button>
        {:else}<Button size="sm" disabled={!!busy} onclick={()=>run('download',api.downloadTranslation)}>下载 / 继续下载</Button>{/if}
        <Button size="sm" variant="outline" disabled={!!busy} onclick={()=>local().catch(e=>error=String(e))}>导入 GGUF</Button>
      </div>
      <details><summary>来源与完整性</summary><p><a href={model.source} target="_blank" rel="noreferrer">腾讯官方模型</a></p><code>Revision: {model.revision}<br/>SHA-256: {model.sha256}</code><p>本地导入当前接受已验证的官方 Q4_K_M 文件。其他量化版本需单独验证。</p></details>
    </article>
  {/each}
  {#if busy}<p aria-live="polite">正在处理：{busy}…</p>{/if}
  {#if busy==='download'}<Button size="sm" variant="outline" onclick={()=>api.cancelTranslationDownload()}>取消下载</Button>{/if}
  {#if progress&&progress.state!=='complete'}<p>{progress.state} · {((progress.received??0)/1024**2).toFixed(0)} / {((progress.total??0)/1024**2).toFixed(0)} MiB</p><progress value={progress.received??0} max={progress.total??1}></progress>{/if}
  {#if error}<pre role="alert">{error}</pre>{/if}
  {#if status?.message}<pre>{status.message}</pre>{/if}
</section>
<style>
  .translation-models{padding:20px 26px}h3{margin:0 0 12px;font-size:15px}p{font-size:12px;line-height:1.7;color:var(--muted)}article{border:1px solid var(--border);padding:16px;border-radius:8px}.row{flex-wrap:wrap}details{margin-top:14px;font-size:11px}code{display:block;overflow-wrap:anywhere}pre{white-space:pre-wrap;overflow-wrap:anywhere;color:#e2818b;font-size:11px;max-height:150px;overflow:auto}progress{width:100%}
</style>
