import {build} from 'esbuild'
import ts from 'typescript'
import {createHash} from 'node:crypto'
import {execFileSync} from 'node:child_process'
import {existsSync,mkdirSync,readFileSync,readdirSync,realpathSync,writeFileSync} from 'node:fs'
import {dirname,extname,isAbsolute,relative,resolve} from 'node:path'
import {fileURLToPath} from 'node:url'

// 只打包明确指定的受控来源，不生成批准配置、key 或默认可用来源。
const project=realpathSync(fileURLToPath(new URL('..',import.meta.url)))
const sha256=bytes=>createHash('sha256').update(bytes).digest('hex')
function inside(path){
 const rel=relative(project,path)
 if(rel==='..'||rel.startsWith('..\\')||rel.startsWith('../')||isAbsolute(rel))throw Error('registration source paths must remain inside the project')
 return rel.replaceAll('\\','/')
}
function outputPath(value){
 const path=resolve(project,value);inside(path)
 let ancestor=path;while(!existsSync(ancestor))ancestor=dirname(ancestor)
 inside(realpathSync(ancestor));return path
}
function standalone(text){
 const file=ts.createSourceFile('browser-registration.js',text,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS)
 function visit(node){
  if(ts.isImportDeclaration(node)||ts.isExportDeclaration(node)&&node.moduleSpecifier||ts.isCallExpression(node)&&node.expression.kind===ts.SyntaxKind.ImportKeyword)throw Error('registration source must be a standalone module')
  ts.forEachChild(node,visit)
 }
 visit(file)
}
async function main(){
 const args=process.argv.slice(2),options={}
 for(let index=0;index<args.length;index+=2){
  const key=args[index],value=args[index+1]
  if(!['--entry','--out-dir'].includes(key)||!value||value.startsWith('--')||key in options)throw Error('usage: --entry <controlled-source.ts> --out-dir <empty-project-directory>')
  options[key]=value
 }
 if(!options['--entry']||!options['--out-dir'])throw Error('usage: --entry <controlled-source.ts> --out-dir <empty-project-directory>')
 const entry=realpathSync(resolve(project,options['--entry']));inside(entry)
 const out=outputPath(options['--out-dir'])
 if(existsSync(out)&&readdirSync(out).length)throw Error('registration source output directory must be empty')
 const sources=new Map()
 const result=await build({entryPoints:[entry],absWorkingDir:project,outfile:resolve(out,'browser-registration.js'),bundle:true,platform:'browser',format:'esm',target:'es2021',write:false,metafile:true,logLevel:'silent',plugins:[{
  name:'capture-registration-source',setup(builder){builder.onLoad({filter:/.*/},args=>{
   const path=realpathSync(args.path),name=inside(path),bytes=readFileSync(path),extension=extname(path)
   const loader={'.ts':'ts','.tsx':'tsx','.js':'js','.mjs':'js','.cjs':'js','.json':'json'}[extension]
   if(!loader)throw Error('unsupported registration source input')
   sources.set(name,{path:name,sha256:sha256(bytes),bytes:bytes.length})
   return{contents:bytes.toString('utf8'),loader,resolveDir:dirname(path)}
  })},
 }]})
 const outputs=Object.values(result.metafile.outputs),file=result.outputFiles[0]
 if(result.warnings.length||outputs.length!==1||result.outputFiles.length!==1||outputs[0].imports.length||outputs[0].exports.length!==1||outputs[0].exports[0]!=='readApprovedBrowserRegistrationInputs'||file.contents.length>1048576)throw Error('registration source requires one bounded standalone reader export')
 standalone(file.text)
 const manifest={version:1,sourceCommit:execFileSync('git',['rev-parse','HEAD'],{cwd:project,encoding:'utf8',windowsHide:true}).trim(),entry:inside(entry),exports:outputs[0].exports,sources:[...sources.values()].sort((a,b)=>a.path.localeCompare(b.path)),artifact:{path:'browser-registration.js',bytes:file.contents.length,sha256:sha256(file.contents)}}
 mkdirSync(out,{recursive:true})
 writeFileSync(resolve(out,'browser-registration.js'),file.contents,{flag:'wx'})
 writeFileSync(resolve(out,'manifest.json'),JSON.stringify(manifest,null,2)+'\n',{flag:'wx'})
 console.log(`Built ${inside(resolve(out,'browser-registration.js'))}; independent approval/provider trust is not established by this build.`)
}
try{await main()}catch(error){console.error(error instanceof Error?error.message:'registration source build failed');process.exitCode=1}
