#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {inspectDirectory,inspectZipFile,writeZip,validId} from './validation.mjs';

export async function main(args){
  const [command,directory='.',...options]=args;
  if(command==='init'){
    const pick=(flag,fallback)=>options.includes(flag)?options[options.indexOf(flag)+1]:fallback;
    const template=pick('--template','tab'),id=pick('--id','author.example');
    if(!['theme','tab','launcher'].includes(template)||!validId(id))throw new Error('Invalid template or ID');
    if(fs.existsSync(directory)&&fs.readdirSync(directory).length)throw new Error('Target directory must be empty');
    fs.mkdirSync(directory,{recursive:true});fs.cpSync(fileURLToPath(new URL(`./templates/${template}/`,import.meta.url)),directory,{recursive:true});
    const manifest=JSON.parse(fs.readFileSync(path.join(directory,'manifest.json')));manifest.id=id;fs.writeFileSync(path.join(directory,'manifest.json'),JSON.stringify(manifest,null,2)+'\n');
    const workflow=path.join(directory,'.github/workflows');fs.mkdirSync(workflow,{recursive:true});fs.copyFileSync(fileURLToPath(new URL('./templates/release.yml',import.meta.url)),path.join(workflow,'release.yml'));
    console.log('Created '+id+'. Use check, pack, or connect this folder in SWCH → Mods.');return;
  }
  if(command==='validate-zip'){console.log(JSON.stringify(await inspectZipFile(directory)));return;}
  if(!['check','pack'].includes(command))throw new Error('Usage: swch-mod init <dir> --template theme|tab|launcher --id author.mod | check <dir> | pack <dir> [output.zip]');
  const {manifest,files}=inspectDirectory(directory);
  if(command==='check'){console.log(JSON.stringify({manifest,files:files.size}));return;}
  const output=path.resolve(options[0]||path.join(directory,'dist',`${manifest.id}-${manifest.version}.zip`));
  console.log(JSON.stringify({...await writeZip(files,output),manifest}));
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){main(process.argv.slice(2)).catch(e=>{console.error(e.message);process.exitCode=1;});}
