<script lang="ts">
  import {onMount,tick} from 'svelte';
  import {invoke} from '@tauri-apps/api/core';
  import {listen} from '@tauri-apps/api/event';
  import type {OverlaySnapshot} from '../types/overlay';
  import {acceptSnapshot,layoutSubtitles,overlayDefaults,safeStyle} from '../lib/overlayGeometry';
  import OverlaySubtitle from './OverlaySubtitle.svelte';
  let snapshot=$state<OverlaySnapshot|null>(null),width=$state(0),height=$state(0);
  const style=$derived(safeStyle(snapshot?.settings??overlayDefaults));
  const layouts=$derived(snapshot?.visible?layoutSubtitles(snapshot.items,snapshot.sourceWidth,snapshot.sourceHeight,width,height,style):[]);
  async function acknowledge(){
    const current=snapshot;if(!current)return;await tick();
    const rect=current.geometry?.rect;
    if(current.visible&&rect&&(Math.abs(innerWidth*devicePixelRatio-(rect[2]-rect[0]))>2||Math.abs(innerHeight*devicePixelRatio-(rect[3]-rect[1]))>2))return;
    if(snapshot?.serial===current.serial)await invoke('overlay_rendered',{serial:current.serial});
  }
  async function receive(next:OverlaySnapshot){
    const accepted=acceptSnapshot(snapshot,next);if(accepted===snapshot)return;snapshot=accepted;
    await acknowledge();
  }
  onMount(()=>{
    let disposed=false;let unlisten:(()=>void)|undefined;
    const resize=()=>{width=innerWidth;height=innerHeight;void acknowledge().catch(()=>{snapshot=null;});};resize();addEventListener('resize',resize);
    void (async()=>{
      unlisten=await listen<OverlaySnapshot>('overlay-state',event=>{void receive(event.payload).catch(()=>{snapshot=null;});});
      if(disposed){unlisten();return;}
      await receive(await invoke<OverlaySnapshot>('overlay_snapshot'));
    })().catch(()=>{snapshot=null;});
    return ()=>{disposed=true;unlisten?.();removeEventListener('resize',resize);};
  });
</script>
<main aria-label="实时翻译字幕" data-serial={snapshot?.serial??0} data-frame={snapshot?.identity?.frameId??0}>
  {#each layouts as item (item.id)}<OverlaySubtitle {item} {style}/>{/each}
</main>
<style>:global(html),:global(body),:global(#app){margin:0;width:100%;height:100%;overflow:hidden;background:transparent!important;color-scheme:normal;pointer-events:none;font-family:"Microsoft YaHei UI","Segoe UI",sans-serif}main{position:fixed;inset:0;overflow:hidden;pointer-events:none}</style>
