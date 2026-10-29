import {describe,it,expect} from 'vitest';
import {acceptSnapshot,blockBounds,layoutSubtitles,mapBox,overlayDefaults,safeStyle,unionBounds} from './overlayGeometry';
import type {OverlaySnapshot} from '../types/overlay';
const box=(bbox:[number,number,number,number])=>({bbox,polygon:null});
const snapshot=(serial:number,revision=1):OverlaySnapshot=>({serial,settings:overlayDefaults,identity:{frameId:serial,generation:1,epoch:1,revision},targetId:'a',sourceWidth:800,sourceHeight:600,geometry:null,items:[{id:'a',text:'新游戏',bbox:[20,30,150,60],sourceBlockIds:['a']}],visible:true,reason:''});
describe('overlay source geometry and layout',()=>{
  it('maps fractional nonuniform scales without adding desktop origins twice',()=>{expect(mapBox([20,30,60,70],100,100,125,150)).toEqual({x:25,y:45,width:50,height:60});expect(mapBox([-10,-20,40,50],100,100,200,50)).toEqual({x:0,y:0,width:80,height:25});});
  it('rejects empty, nonfinite, inverted and entirely out-of-image boxes',()=>{for(const b of [[0,0,0,3],[5,5,2,2],[NaN,0,5,5],[101,0,110,20]])expect(blockBounds(box(b as [number,number,number,number]),100,100)).toBeNull();expect(mapBox([0,0,1,1],0,10,10,10)).toBeNull();});
  it('unions adjacent mixed polygon/bbox blocks but rejects remote or unknown geometry',()=>{
    const p={bbox:null,polygon:[{x:0,y:25},{x:100,y:25},{x:100,y:45},{x:0,y:45}]};expect(unionBounds([box([0,0,100,20]),p],800,600)).toEqual([0,0,100,45]);expect(unionBounds([box([0,0,50,20]),box([700,500,750,520])],800,600)).toBeNull();expect(unionBounds([box([0,0,50,20]),{bbox:null,polygon:null}],800,600)).toBeNull();
  });
  it('constrains mixed Chinese Japanese and long Latin text without collisions',()=>{
    const items=Array.from({length:4},(_,i)=>({id:String(i),text:'中文译文・日本語 LongUnbrokenLatinWord'.repeat(i?20:1),bbox:[680,450+i*20,790,470+i*20] as [number,number,number,number],sourceBlockIds:[]}));
    const layouts=layoutSubtitles(items,800,600,1000,750,overlayDefaults);expect(layouts.length).toBeGreaterThan(0);
    for(const l of layouts){expect(l.fontSize).toBeGreaterThanOrEqual(12);expect(l.lines).toBeLessThanOrEqual(6);expect(l.x).toBeGreaterThanOrEqual(0);expect(l.y).toBeGreaterThanOrEqual(0);expect(l.x+l.width).toBeLessThanOrEqual(1000);expect(l.y+l.height).toBeLessThanOrEqual(750);expect(l.width).toBeLessThanOrEqual(450);expect(l.height).toBeLessThanOrEqual(200);}
  });
  it('bounds unsafe style inputs',()=>{expect(safeStyle({...overlayDefaults,overlayOpacity:NaN,overlayFontSize:1,overlayBlur:100,overlayPadding:-2})).toMatchObject({overlayOpacity:.55,overlayFontSize:12,overlayBlur:16,overlayPadding:2});});
});
describe('overlay event ordering independent from history',()=>{
  it('retains identical content and relocates the same keyed subtitle across frames',()=>{const a=snapshot(1),b=snapshot(2);b.items[0].bbox=[500,30,630,60];const next=acceptSnapshot(a,b)!;expect(next.items[0].id).toBe(a.items[0].id);expect(next.items[0].text).toBe(a.items[0].text);expect(next.items[0].bbox[0]).toBe(500);});
  it('applies empty new-text invalidation and rejects stale revisions and delayed snapshots',()=>{const a=snapshot(3,2);a.items=[];expect(acceptSnapshot(snapshot(2),a)?.items).toEqual([]);expect(acceptSnapshot(a,snapshot(4,1))).toBe(a);expect(acceptSnapshot(a,snapshot(2,2))).toBe(a);const clear={...a,serial:5,identity:null,visible:false};expect(acceptSnapshot(clear,a)).toBe(clear);});
});
