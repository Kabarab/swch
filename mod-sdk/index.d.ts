export interface Game {id:string;app_id?:string;name:string;is_installed:boolean;launcher?:string;size_bytes?:number;install_path?:string;executable_path?:string;banner_url?:string;launch_uri?:string;owner_ids?:string[]}
export interface GameTarget {launcher:string;app_id:string;name?:string}
export interface ModManifest {id:string;name:string;version:string;api:string;platforms:('windows-x64'|'macos-arm64')[];description?:string;games?:GameTarget[];backend?:string;ui?:string;theme?:string}
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
export interface SDK {
 apiVersion:string;library:{list():Promise<Game[]>};accounts:{list():Promise<Account[]>};
 storage:KeyValue;secrets:{get(key:string):Promise<string|null>;set(key:string,value:string):Promise<void>;delete(key:string):Promise<void>};
 settings:{get():Promise<Record<string,unknown>>;set(value:Record<string,unknown>):Promise<void>};
 events:{emit(event:string,data:unknown):void;on(event:string,listener:(data:unknown)=>void|Promise<void>):()=>void};log(...args:unknown[]):void;
 registerLauncher(metadata:LauncherMetadata,handlers:LauncherHandlers):void;
 registerTab(metadata:{id:string;name:string}):void;
 registerAction(metadata:{id:string;name:string},handler:(input:unknown)=>unknown|Promise<unknown>):void;
}
export type Activate=(swch:SDK)=>void|Promise<void>;
/** iframe bridge, supplied as window.swch by SWCH. */
export interface UIBridge {tab:string;game:Game|null;account:Account|null;call(method:string,params?:unknown):Promise<unknown>;on(event:string,listener:(data:unknown)=>void):()=>void}
declare global {interface Window {swch:UIBridge}}
