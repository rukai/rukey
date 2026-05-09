# Rukey

## Problems with rev 1 (current)

Its completely broken..

* idc socket collides with switches
* TP* shorted on pico via plated through holes - very dangerous to the host
* longest screw hole needs to be longer
* longest screw hole on pcb is too close to the pico

<!-- old

## TODO

* 3DP
  * add screw hole in between the 2 idc connectors
  * move thumb cluster closer to thumb
  * Add a switch plate
* PCB
  * Make board 4mm taller
  * Shift left-half idc connector to the left 5.0mm and up 4mm
  * Move pico up 4mm
  * add screw hole in between the 2 idc connectors
  * ensure routing around longest screw hole is far away from the hole
  * make pico into a throughhole footprint

## Done

* 3DP
  * longest screw needs to be longer
  * Make case 4mm taller (top-view)
* PCB
-->

## TODO

* 3DP
  * add screw hole in between the 2 idc connectors
  * move thumb cluster closer to thumb
  * Add a switch plate
* PCB
  * move left-half idc connector to the right of the pico + rotate
  * move right-half idc connector to the right of the pico + rotate
  * add screw hole top right corner, left of pico
  * move bottom right screw hole in between keys
  * maybe: make pico into a throughhole footprint and move right-half idc connector under pico

## Done

* 3DP
  * longest screw needs to be longer
  * Make case 4mm taller (top-view)
* PCB

## About

This was designed quickly as my personal keyboard, I would like to make this a properly documented project that others can build but don't have the capacity at the moment.

Used the sofle pico as a base but changed pretty much everything, in order to recreate the moonlander but with the goal of being a budget build.

## BOM

[Printed parts onshape](https://cad.onshape.com/documents/f6585266405bfc96ac0755a6/w/eb9972e583e4bd12c4cf19cb/e/e24845aae63ec51d7ef00fa9?renderMode=0&uiState=69eeedeaa3d4f3ef21615389)

|Quantity|Item|Note|
|-|-|-|
|74| MX compatible switches| |
|74| MX compatible kailh hotswap sockets| |
|74| Surface mount SOD-123 1N4148 diodes| |
|2| 2x08 2.00mm IDC straight socket| |
|1| 2x08 2.00mm IDC cable | |
|4| 2x03 2.00mm IDC straight socket | |
|2| 2x03 2.00mm IDC cable 10cm | |
|1| printed left base|print with supports for the bridge, can be done with auto supports configured to 1 degree|
|1| printed right base| same as above|
|8| printed washers| |
|8| m3 machine screws of various lengths|TODO: document actual sizes |
|8| m3 square nuts| |
|1| main PCB| |
|1| thumb cluster PCB| |
