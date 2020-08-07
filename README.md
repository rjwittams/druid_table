# druid_table

A table/grid widget for druid ( the rust widget library). WIP.
This project was to learn rust.

* Virtualized rows and columns, ie data does not have to reside in memory and there is nothing stored per row/column/cell (if fixed sizes are used).
* Custom cell and header rendering
* Columns can be static or data derived
* Selections (single cell + row & column) with keyboard control
* Sorting (multi column, asc/desc) - up front specified right now. Interactive by double clicking in column headers (CTRL for multi select).
* Trait based design for customisation (and possible monomorphisation benefits) :
    * Data sources:
        * Currently im::Vector is supported out of the box.
        * The interface works for both virtualized and concrete data sources by reference. Minimum copying required. 
    * Columns:
        * The default configured columns that are boxed onto the heap, allowing composition. Columns can be adapted with lenses or functions (to pull fields out of your row type). 
        * Implement your own "CellsDelegate" if configuration up front won't work - for example deriving columns from the data (DB result sets).
    * Axes: 
        * The default AxisMeasures allow user driven column resizing - right now this takes up O(n) memory in items on that axis. 
        * The FixedAxisMeasure stores no data, so is better for huge virtual tables. See examples/bigtable - tested on a quintillion cells!

Planned:
  * Fuller configuration (improved builder)
  * Filter
  * Pushdown of sort and filter (eg db backend)
  * Column and row pinning
  * Editing
  * Selection/ clipboard
  * WASM / + JS wrapper (aspirational)
  * Support slow data sources (loading)
  * Support fast data sources (ticking)
  
Later: 
  * Reduce memory usage of resizable columns to O(n) in the number of columns resized
  * Aggregation (w/push down) 
  * Pivoting (w/push down)
  * Tree view - not sure if this is just a weird column that hides rows

Much later:
  * More optimised data representations for large in memory datasets - possibly MVCC and columnar. Maybe an add on.

Requires my own druid [fork](https://github.com/rjwittams/druid) until [1108](https://github.com/linebender/druid/pull/1108) is merged

This shows it in action and also a bug where the header resize continues when leaving the widget. Thats been fixed!
![ezgif-4-bbc742141fc1](https://user-images.githubusercontent.com/752137/89051955-e7e8f280-d34c-11ea-85ca-175f3e291ced.gif)

Can be considered for inclusion when its a bit further along.
