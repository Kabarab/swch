export interface Game {id:string;app_id?:string;name:string;is_installed:boolean;launcher?:string;size_bytes?:number;install_path?:string;executable_path?:string;banner_url?:string;launch_uri?:string;owner_ids?:string[]}
export interface GameTarget {launcher:string;app_id:string;name?:string}
export interface NativeToolFile {path:string;sha256:string;executable?:boolean}
export interface NativeTool {id:string;platform:'windows-x64'|'macos-arm64';version:string;license:string;files:NativeToolFile[]}
export interface ModManifest {id:string;name:string;version:string;api:string;platforms:('windows-x64'|'macos-arm64')[];description?:string;cover_url?:string;games?:GameTarget[];backend?:string;ui?:string;theme?:string;native_tools?:NativeTool[];[extension:string]:unknown}
export interface Account {id:string;username:string;launcher?:string;display_name?:string;email?:string;is_active:boolean;last_used?:string}
export interface KeyValue {get(key:string):Promise<unknown>;set(key:string,value:unknown):Promise<void>;delete(key:string):Promise<void>}
export type Operation='library'|'sync'|'accounts'|'connect'|'disconnect'|'switch'|'launch';
export interface LauncherMetadata {id:string;name:string;capabilities:Operation[]}
export interface LauncherHandlers {
 library?():Promise<Game[]>;sync?():Promise<Game[]>;accounts?():Promise<Account[]>;
 connect?(input:Record<string,unknown>):Promise<unknown>;
 disconnect?(input:{account_id:string}):Promise<unknown>;
 switch?(input:{account_id:string}):Promise<unknown>;
 launch?(input:{game_id:string;account_id?:string}):Promise<unknown>;
}
export type FileKind='data'|'cache'|'temp'|'tools';
export type FilePaths=Record<FileKind,string>;
export type FileUsage=Record<FileKind,number>;
export interface FileDialogOptions {title?:string;multiple?:boolean;extensions?:string[]}
export interface ToolProcess {process_id:number;tool_id:string;running:boolean;exit_code?:number|null}
export interface InstalledTool extends NativeTool {directory:string}
export type JobStatus='queued'|'running'|'completed'|'failed'|'cancelled';
export interface Job<Input=unknown,Result=unknown> {
 id:string;type:string;status:JobStatus;input:Input;progress:unknown;checkpoint:unknown;result:Result|null;error:string|null;created_at:string;updated_at:string;attempt:number;
}
export interface JobContext {
 id:string;signal:AbortSignal;attempt:number;savedCheckpoint:unknown;
 progress(value:unknown):Promise<Job>;checkpoint(value:unknown):Promise<Job>;
}
export type JobHandler<Input=unknown,Result=unknown>=(input:Input,context:JobContext)=>Result|Promise<Result>;
export interface Jobs {
 register<Input=unknown,Result=unknown>(type:string,handler:JobHandler<Input,Result>):void;
 start<Input=unknown,Result=unknown>(type:string,input?:Input):Promise<Job<Input,Result>>;
 list():Promise<Job[]>;get(id:string):Promise<Job>;cancel(id:string):Promise<Job>;
}
export interface SDK {
 apiVersion:string;library:{list():Promise<Game[]>;refreshSteam(input?:{account_id?:string}):Promise<unknown>};accounts:{list():Promise<Account[]>};
 storage:KeyValue;secrets:{get(key:string):Promise<string|null>;set(key:string,value:string):Promise<void>;delete(key:string):Promise<void>};
 settings:{get():Promise<Record<string,unknown>>;set(value:Record<string,unknown>):Promise<void>};
 files:{paths():Promise<FilePaths>;usage():Promise<FileUsage>;configure(kind:FileKind,directory:string):Promise<FilePaths>;clearCache():Promise<void>};
 dialogs:{openFile(input?:FileDialogOptions):Promise<string[]>;openFolder(input?:{title?:string}):Promise<string|null>};
 shell:{openFolder(path:string):Promise<void>};clipboard:{readText():Promise<string>;writeText(text:string):Promise<void>};
 tools:{list():Promise<InstalledTool[]>;run(id:string,args?:string[],options?:{executable?:string}):Promise<{process_id:number}>;status():Promise<ToolProcess[]>;stop(process_id:number):Promise<void>};
 jobs:Jobs;
 events:{emit(event:string,data:unknown):void;on(event:string,listener:(data:unknown)=>void|Promise<void>):()=>void};log(...args:unknown[]):void;
 registerLauncher(metadata:LauncherMetadata,handlers:LauncherHandlers):void;
 registerTab(metadata:{id:string;name:string}):void;
 registerAction(metadata:{id:string;name:string},handler:(input:unknown)=>unknown|Promise<unknown>):void;
 registerMethod(id:string,handler:(input:unknown)=>unknown|Promise<unknown>):void;
}
export type Cleanup=()=>void|Promise<void>;
export type Activate=(swch:SDK)=>void|Cleanup|Promise<void|Cleanup>;
/** iframe bridge, supplied as window.swch by SWCH. */
export interface UIBridge {tab:string;game:Game|null;account:Account|null;assets:{url(path:string):string};call(method:string,params?:unknown):Promise<unknown>;on(event:string,listener:(data:unknown)=>void):()=>void}
declare global {interface Window {swch:UIBridge}}
