export namespace models {
	
	export class Account {
	    id: string;
	    displayName: string;
	    username: string;
	    platform: string;
	    avatarUrl: string;
	
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
	    }
	}
	export class AccountStat {
	    accountId: string;
	    displayName: string;
	    playtimeMin: number;
	    lastPlayed: number;
	
	    static createFrom(source: any = {}) {
	        return new AccountStat(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.accountId = source["accountId"];
	        this.displayName = source["displayName"];
	        this.playtimeMin = source["playtimeMin"];
	        this.lastPlayed = source["lastPlayed"];
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

