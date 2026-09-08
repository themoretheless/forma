import {createStudioControlsProject} from './studio-controls-project.js';
const library=import.meta.glob('../vector-ui/controls/{components,assets}/*',{query:'?raw',import:'default',eager:true});
const sources=import.meta.glob('../studio-controls/{components,ui}/*.ui',{query:'?raw',import:'default',eager:true});
const notes=import.meta.glob('../studio-controls/README.md',{query:'?raw',import:'default',eager:true});
const paths=(files,prefix)=>Object.fromEntries(Object.entries(files).map(([path,source])=>[path.slice(prefix.length),source]));
export const studioControlsProject=createStudioControlsProject(paths(library,'../vector-ui/controls/'),{...paths(sources,'../studio-controls/'),...paths(notes,'../studio-controls/')});
