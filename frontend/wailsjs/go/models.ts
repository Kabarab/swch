export namespace models {
	
	export class Game {
	    id: string;
	    name: string;
	    platform: string;
	    imageUrl: string;
	
	    static createFrom(source: any = {}) {
	        return new Game(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.id = source["id"];
	        this.name = source["name"];
	        this.platform = source["platform"];
	        this.imageUrl = source["imageUrl"];
	    }
	}
	export class Account {
	    id: string;
	    displayName: string;
	    username: string;
	    platform: string;
	    avatarUrl: string;
	    ownedGames: Game[];
	    comment: string;
	
	    static createFrom(source: any = {}) {
	        return new Account(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.id = source["id"];
	        this.displayName = source["displayName"];
	        this.username = source["username"];
	        this.platform = source["platform"];
	        this.avatarUrl = source["avatarUrl"];
	        this.ownedGames = this.convertValues(source["ownedGames"], Game);
	        this.comment = source["comment"];
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}
	export class AccountStat {
	    accountId: string;
	    displayName: string;
	    username: string;
	    playtimeMin: number;
	    lastPlayed: number;
	    note: string;
	    isHidden: boolean;
	
	    static createFrom(source: any = {}) {
	        return new AccountStat(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.accountId = source["accountId"];
	        this.displayName = source["displayName"];
	        this.username = source["username"];
	        this.playtimeMin = source["playtimeMin"];
	        this.lastPlayed = source["lastPlayed"];
	        this.note = source["note"];
	        this.isHidden = source["isHidden"];
	    }
	}
	
	export class GameUI {
	    id: string;
	    title: string;
	    image: string;
	    installed: boolean;
	    source: string;
	
	    static createFrom(source: any = {}) {
	        return new GameUI(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.id = source["id"];
	        this.title = source["title"];
	        this.image = source["image"];
	        this.installed = source["installed"];
	        this.source = source["source"];
	    }
	}
	export class LauncherGroup {
	    name: string;
	    platform: string;
	    accounts: Account[];
	
	    static createFrom(source: any = {}) {
	        return new LauncherGroup(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.name = source["name"];
	        this.platform = source["platform"];
	        this.accounts = this.convertValues(source["accounts"], Account);
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}
	export class LibraryGame {
	    id: string;
	    name: string;
	    platform: string;
	    iconUrl: string;
	    exePath: string;
	    availableOn: AccountStat[];
	    isInstalled: boolean;
	    isPinned: boolean;
	    isMacSupported: boolean;
	
	    static createFrom(source: any = {}) {
	        return new LibraryGame(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.id = source["id"];
	        this.name = source["name"];
	        this.platform = source["platform"];
	        this.iconUrl = source["iconUrl"];
	        this.exePath = source["exePath"];
	        this.availableOn = this.convertValues(source["availableOn"], AccountStat);
	        this.isInstalled = source["isInstalled"];
	        this.isPinned = source["isPinned"];
	        this.isMacSupported = source["isMacSupported"];
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}

}

