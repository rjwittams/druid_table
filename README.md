# druid_table

A table widget for druid ( the rust widget library). WIP.

* Virtualized rows and columns, ie data does not have to reside in memory and there is nothing stored per row/column/cell (if fixed sizes are used).
* Custom cell and header rendering
* Columnns can be static or data derived
* Selections (single cell right now)

Requires my own druid [fork](https://github.com/rjwittams/druid) until [1108](https://github.com/linebender/druid/pull/1108) is merged

This shows it in action and also a bug where the header resize continues when leaving the widget
![ezgif-4-bbc742141fc1](https://user-images.githubusercontent.com/752137/89051955-e7e8f280-d34c-11ea-85ca-175f3e291ced.gif)

Can be considered for inclusion when its a bit further along.
