import type {RetirableNativeConnection} from './native.ts';
import type {Target} from './reducer.ts';
export type TerminalConnection=Readonly<{connection:RetirableNativeConnection;target:Target;binding:string;role:'Controller'|'Viewer'}>;
/** A source owns its original setup until settlement, including abort/late results. */
export type TerminalConnectionSource=Readonly<{open(signal:AbortSignal,setupDeadline?:number):Promise<TerminalConnection|undefined>}>;
