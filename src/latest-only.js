// Tracks which async load is current: begin() starts a new generation and returns a check that is
// true only while no newer load began and cancel() was not called, so a late result builds nothing.
export function createLatestOnly(){
 let generation=0;
 return {
  begin(){const own=++generation;return ()=>own===generation;},
  cancel(){generation++;},
 };
}
