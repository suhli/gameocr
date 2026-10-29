import { mount } from 'svelte';
const target=document.getElementById('app')!;
if(new URLSearchParams(location.search).has('overlay')){
  const {default:OverlayApp}=await import('./overlay/OverlayApp.svelte');
  mount(OverlayApp,{target});
}else{
  const {default:App}=await import('./App.svelte');
  mount(App,{target});
}
