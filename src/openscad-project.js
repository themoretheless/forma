import {materializeTheme} from '../vector-ui/controls/tokens.js';

export function createOpenScadProject(library,sources){
 const entry='ui/OpenScadStudio.ui';
 const merged=materializeTheme({...library,...sources},'dark');
 const order=[
  entry,
  'ui/OpenScadStudio.design.ui',
  ...Object.keys(sources).filter(path=>path.startsWith('ui/')&&path!==entry&&path!=='ui/OpenScadStudio.design.ui'),
  ...Object.keys(sources).filter(path=>path.startsWith('src/')),
  'Cargo.toml',
  ...Object.keys(merged),
 ];
 return Object.fromEntries([...new Set(order)].filter(path=>path in merged).map(path=>[path,merged[path]]));
}
