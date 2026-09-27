import {materializeTheme} from '../vector-ui/controls/tokens.js';

export function createStudioControlsProject(library,sources){
 const entry='ui/StudioControls.ui';
 const merged=materializeTheme({...library,...sources},'dark');
 const order=[entry,...Object.keys(sources).filter(path=>path.startsWith('ui/')),...Object.keys(sources).filter(path=>path.startsWith('components/')),...Object.keys(merged)];
 return Object.fromEntries([...new Set(order)].map(path=>[path,merged[path]]));
}
