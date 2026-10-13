// Inspect PE imports without relying on DLLs in the developer's PATH.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const root=path.resolve(process.argv[2]||'artifacts/GameOCR-windows-x64-portable');
function imports(file){
  const b=fs.readFileSync(file), pe=b.readUInt32LE(0x3c);
  assert.equal(b.toString('ascii',pe,pe+4),'PE\0\0');
  const count=b.readUInt16LE(pe+6),optional=pe+24,magic=b.readUInt16LE(optional);
  assert.equal(magic,0x20b,`${file}: expected x64 PE`);
  const table=optional+b.readUInt16LE(pe+20), directories=optional+112;
  function offset(rva){
    for(let i=0;i<count;i++){const s=table+i*40,start=b.readUInt32LE(s+12),size=Math.max(b.readUInt32LE(s+8),b.readUInt32LE(s+16));if(rva>=start&&rva<start+size)return b.readUInt32LE(s+20)+rva-start;}
    throw Error(`Invalid PE address in ${file}: ${rva}`);
  }
  function string(rva){const p=offset(rva);return b.toString('ascii',p,b.indexOf(0,p));}
  const names=[];
  for(const [index,stride,nameOffset] of [[1,20,12],[13,32,4]]){
    const rva=b.readUInt32LE(directories+index*8);if(!rva)continue;
    for(let p=offset(rva);b.readUInt32LE(p+nameOffset);p+=stride)names.push(string(b.readUInt32LE(p+nameOffset)));
  }
  return names;
}
const files=[];
function walk(dir){for(const item of fs.readdirSync(dir,{withFileTypes:true})){const p=path.join(dir,item.name);if(item.isDirectory())walk(p);else if(/\.(exe|dll)$/i.test(item.name))files.push(p);}}
walk(root);
const report=[];
for(const file of files){
  const deps=imports(file);
  if(path.basename(file).toLowerCase()==='gameocr.exe')assert(!deps.some(d=>/cuda|cublas|ggml/i.test(d)),'UI must not import CUDA');
  for(const dep of deps){
    const available=/^(api-ms-|ext-ms-)/i.test(dep)||[path.dirname(file),root,path.join(process.env.SystemRoot,'System32')].some(d=>fs.existsSync(path.join(d,dep)));
    assert(available,`${path.relative(root,file)} requires missing ${dep}`);
  }
  report.push({file:path.relative(root,file),imports:deps});
}
fs.writeFileSync('artifacts/native-imports.json',JSON.stringify(report,null,2));
console.log(`Verified imports for ${files.length} x64 binaries using package paths and system32 only.`);
