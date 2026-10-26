import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
const sections=[];
function collect(name,version,license,root,repository=''){
  const texts=[];
  if(fs.existsSync(root))for(const file of fs.readdirSync(root)){
    const p=path.join(root,file);
    if(/^(licen[sc]e|copying|notice)([.-]|$)/i.test(file)&&fs.statSync(p).isFile())texts.push(`${file}\n${fs.readFileSync(p,'utf8')}`);
  }
  sections.push(`${'='.repeat(72)}\n${name} ${version}\nSPDX: ${license||'See upstream license'}\n${repository}\n\n${texts.join('\n\n')||'See package metadata and upstream repository for license text.'}`);
}
const metadata=JSON.parse(execFileSync('cargo',['metadata','--locked','--format-version','1'],{encoding:'utf8',maxBuffer:50*1024*1024}));
for(const pkg of metadata.packages)if(pkg.source)collect(pkg.name,pkg.version,pkg.license,path.dirname(pkg.manifest_path),pkg.repository);
const lock=JSON.parse(fs.readFileSync('package-lock.json','utf8'));
// Include build dependencies too: Svelte's runtime is compiled into the frontend bundle.
for(const [location] of Object.entries(lock.packages))if(location){
  const file=path.join(location,'package.json');
  if(fs.existsSync(file)){const info=JSON.parse(fs.readFileSync(file,'utf8'));collect(info.name,info.version,info.license,location,typeof info.repository==='string'?info.repository:info.repository?.url);}
}
const sys=metadata.packages.find(p=>p.name==='llama-cpp-sys-2');
if(sys)collect('llama.cpp / ggml (vendored)',sys.version,'MIT',path.join(path.dirname(sys.manifest_path),'llama.cpp'),'https://github.com/ggml-org/llama.cpp');
fs.mkdirSync('src-tauri/resources/licenses',{recursive:true});
fs.writeFileSync('src-tauri/resources/licenses/dependencies.txt',sections.join('\n\n'));
fs.copyFileSync('THIRD-PARTY-NOTICES.md','src-tauri/resources/licenses/THIRD-PARTY-NOTICES.md');
fs.copyFileSync('LICENSE','src-tauri/resources/licenses/GameOCR-LICENSE.txt');
fs.copyFileSync('licenses/Hy-MT2-LICENSE.txt','src-tauri/resources/licenses/Hy-MT2-LICENSE.txt');
console.log(`Collected ${sections.length} dependency license notices.`);
