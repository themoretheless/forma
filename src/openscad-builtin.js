import {createOpenScadProject} from './openscad-project.js';
const library=import.meta.glob('../vector-ui/controls/{components,assets}/*',{query:'?raw',import:'default',eager:true});
const sources=import.meta.glob('../openscad-ui/ui/*.ui',{query:'?raw',import:'default',eager:true});
const rust=import.meta.glob('../openscad-ui/{Cargo.toml,src/*.rs}',{query:'?raw',import:'default',eager:true});
const paths=(files,prefix)=>Object.fromEntries(Object.entries(files).map(([path,source])=>[path.slice(prefix.length),source]));
export const openScadProject=createOpenScadProject(paths(library,'../vector-ui/controls/'),{...paths(sources,'../openscad-ui/'),...paths(rust,'../openscad-ui/')});
