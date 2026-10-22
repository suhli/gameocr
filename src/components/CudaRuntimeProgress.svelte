<script lang="ts">
  import { onMount } from 'svelte';
  import { api,Subscriptions,type CudaRuntimeProgress } from '../services/api';
  import { Button } from '$lib/components/ui/button';
  let {onError}:{onError:(e:unknown)=>void}=$props();
  let progress=$state<CudaRuntimeProgress|null>(null),cancelling=$state(false);
  const active=$derived(progress&&['checking','downloading','verifying','retrying','extracting'].includes(progress.state));
  const labels:Record<string,string>={checking:'检查 CUDA 运行库',downloading:'下载 CUDA 运行库',verifying:'校验下载文件',retrying:'重试下载',extracting:'准备 CUDA 运行库'};
  onMount(()=>{
    const events=new Subscriptions();let disposed=false,receivedEvent=false;
    void (async()=>{
      await events.add('cuda-runtime-progress',value=>{receivedEvent=true;progress=value;if(!['checking','downloading','verifying','retrying','extracting'].includes(value.state))cancelling=false;});
      const current=await api.cudaRuntimeStatus();if(!disposed&&!receivedEvent)progress=current;
    })().catch(onError);
    return()=>{disposed=true;events.dispose();};
  });
  async function cancel(){cancelling=true;try{await api.cancelCudaDownload();}catch(e){cancelling=false;onError(e);}}
</script>
{#if active&&progress}
  <div class="runtime-progress" role="status" aria-label="CUDA 运行库下载">
    <div><strong>{labels[progress.state]}{progress.component?` · ${progress.component}`:''}</strong><p>{progress.message}</p>
      {#if progress.total>0}<progress aria-label="CUDA 下载进度" value={progress.received} max={progress.total}></progress><small>{(progress.received/1024/1024).toFixed(0)} / {(progress.total/1024/1024).toFixed(0)} MB · NVIDIA 官方源 · 支持续传</small>{/if}
    </div>
    <Button variant="outline" size="sm" disabled={cancelling} onclick={cancel}>{cancelling?'正在取消…':'取消下载'}</Button>
  </div>
{/if}
<style>
  .runtime-progress{display:flex;align-items:center;gap:20px;padding:14px 22px;border-bottom:1px solid var(--border);background:var(--panel);flex-shrink:0}.runtime-progress>div{flex:1;min-width:0}strong{font-size:12px}p{font-size:11px;color:var(--muted);margin:7px 0}progress{display:block;width:100%;height:6px;accent-color:var(--primary);margin:8px 0}small{font-size:10px;color:var(--muted)}
</style>
