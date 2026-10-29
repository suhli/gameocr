<script lang="ts">
  import {onMount} from 'svelte';
  import {api, native, Subscriptions} from '../services/api';
  import type {OverlaySettings} from '../types/overlay';
  let {onError, onSettings}:{onError:(error:unknown)=>void;onSettings:(settings:OverlaySettings)=>void}=$props();
  let settings=$state<OverlaySettings|null>(null),reason=$state(''),busy=$state(false);
  function apply(next:OverlaySettings){settings=next;onSettings(next);}
  async function toggle(){
    if(!settings)return;busy=true;
    try{
      // Read the latest saved style so a quick toggle cannot overwrite changes
      // made in the full settings dialog.
      const latest=await api.overlaySettings();
      const next={...latest,overlayEnabled:!latest.overlayEnabled};
      await api.updateOverlaySettings(next);apply(next);
    }catch(error){onError(error);}finally{busy=false;}
  }
  onMount(()=>{
    if(!native)return;
    const events=new Subscriptions();let disposed=false,changed=false,statusChanged=false;
    void (async()=>{
      await Promise.all([
        events.add('overlay-settings-changed',next=>{changed=true;apply(next);}),
        events.add('overlay-status',next=>{statusChanged=true;reason=next;}),
      ]);
      const [saved,snapshot]=await Promise.all([api.overlaySettings(),api.overlaySnapshot()]);
      if(disposed)return;if(!changed)apply(saved);if(!statusChanged)reason=snapshot.reason;
    })().catch(onError);
    return()=>{disposed=true;events.dispose();};
  });
</script>
<div class="overlay-control">
  <button class:enabled={settings?.overlayEnabled} aria-label="游戏窗口字幕" aria-pressed={settings?.overlayEnabled??false} disabled={!native||!settings||busy} onclick={toggle} title="把译文叠加到当前游戏窗口；切回游戏后显示">
    {busy?'应用中…':settings?.overlayEnabled?'游戏字幕：开':'游戏字幕：关'}
  </button>
  <span aria-label="游戏字幕状态" title={reason}>{settings?.overlayEnabled?(reason||'切回游戏窗口显示译文'):'预览译文可见；游戏字幕需单独开启'}</span>
</div>
<style>
  .overlay-control{display:flex;align-items:center;gap:8px;font-size:11px;min-width:0}
  button{flex-shrink:0;border:1px solid var(--border);background:var(--background);color:var(--foreground);border-radius:6px;padding:6px 10px}
  button.enabled{color:var(--primary);border-color:var(--primary)}button:disabled{opacity:.5}
  span{color:var(--muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
</style>
