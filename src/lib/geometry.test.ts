import { describe,it,expect } from 'vitest';
import { boxStyle,fitWidth } from './geometry';
describe('原图坐标与缩放',()=>{
  it('按原始宽高分别映射到百分比',()=>{expect(boxStyle([192,108,960,540],1920,1080)).toBe('left:10%;top:10%;width:40%;height:40%');});
  it('宽高任一方向变化仍保持比例',()=>{expect(fitWidth(1920,1080,1008,588)).toBe(960);expect(fitWidth(1920,1080,700,300)).toBe(448);});
});
