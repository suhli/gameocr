import {chromium} from '@playwright/test';import fs from 'node:fs';import path from 'node:path';import {pathToFileURL} from 'node:url';import assert from 'node:assert/strict';import {execFileSync} from 'node:child_process';
const root=path.resolve('.cache/annotator-test-'+Date.now());fs.mkdirSync(root,{recursive:true});
for(const name of ['static-dialog-0.png','static-dialog-1.png'])fs.copyFileSync(path.join('tests/fixtures/temporal',name),path.join(root,name));
const browser=await chromium.launch({channel:'msedge',headless:true});
try{
 const page=await browser.newPage({viewport:{width:1440,height:1100}});await page.goto(pathToFileURL(path.resolve('scripts/temporal-annotator.html')).href);
 await page.locator('#files').setInputFiles([path.join(root,'static-dialog-0.png'),path.join(root,'static-dialog-1.png')]);await page.locator('#status').filter({hasText:'1 / 2'}).waitFor();
 const box=await page.locator('canvas').boundingBox();await page.mouse.move(box.x+70,box.y+90);await page.mouse.down();await page.mouse.move(box.x+310,box.y+145,{steps:5});await page.mouse.up();await page.getByLabel('文字 1').fill('annotation workflow test');await page.locator('#checked').check();
 await page.locator('#next').click();await page.locator('#checked').check();const download=page.waitForEvent('download');await page.locator('#export').click();const file=await download;await file.saveAs(path.join(root,'annotations.json'));
 const data=JSON.parse(fs.readFileSync(path.join(root,'annotations.json'),'utf8'));assert.equal(data.scenarios[0].frames.length,2);assert.equal(data.scenarios[0].frames[0].groundTruth[0].text,'annotation workflow test');
 execFileSync(process.execPath,['scripts/import-temporal-corpus.mjs',path.join(root,'annotations.json'),path.join(root,'imported')],{windowsHide:true});
 const imported=JSON.parse(fs.readFileSync(path.join(root,'imported/corpus.json'),'utf8'));assert.equal(imported.scenarios[0].frames[0].sha256.length,64);
 await page.screenshot({path:path.join(root,'annotator.png')});fs.writeFileSync('docs/test-results/temporal-annotation.json',JSON.stringify({executedAt:new Date().toISOString(),scope:'UI/import workflow only; test strings are not OCR ground truth',checks:['draw source-coordinate box','edit text','review each frame','export ordered annotations','import with SHA-256']},null,2)+'\n');
}finally{await browser.close();}
