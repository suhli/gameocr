<script lang="ts">
  import { onMount } from 'svelte';
  import { X } from '@lucide/svelte';
  let {src,onClose}:{src:string;onClose:()=>void}=$props();
  let dialog:HTMLDialogElement;
  onMount(()=>dialog.showModal());
</script>
<dialog bind:this={dialog} oncancel={onClose}>
  <header class="row spread"><strong>原始截图 · 100%</strong><button class="icon-btn" aria-label="关闭原始截图" onclick={onClose}><X/></button></header>
  <div class="image-scroll"><img {src} alt="完整分辨率的原始截图"/></div>
</dialog>
<style>
  dialog{max-width:92vw;max-height:90vh;padding:0;border:1px solid var(--border);border-radius:10px;background:var(--panel);color:var(--foreground)}
  dialog::backdrop{background:#000a}header{padding:12px 20px;border-bottom:1px solid var(--border);font-size:12px}
  .image-scroll{overflow:auto;max-width:90vw;max-height:78vh}img{max-width:none;display:block}
</style>
