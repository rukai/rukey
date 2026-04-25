#!/bin/sh

cd "$(dirname "$0")"

set -e

OUT=target/kicad/rukey_main_gerber_files
ZIP=rukey_main_gerber_files.zip
PCB=PCB_main/Sofle_Pico.kicad_pcb

rm -f $ZIP

mkdir -p $OUT
kicad-cli pcb export drill --output $OUT $PCB
kicad-cli pcb export gerbers --output $OUT $PCB
cd target/kicad
zip $ZIP rukey_main_gerber_files/*
mv $ZIP ../..
cd -

kicad-cli pcb export step --subst-models --output rukey_main_pcb.step $PCB


OUT=target/kicad/rukey_thumb_cluster_gerber_files
ZIP=rukey_thumb_cluster_gerber_files.zip
PCB=PCB_thumb_cluster/Sofle_Pico.kicad_pcb

rm -f $ZIP

mkdir -p $OUT
kicad-cli pcb export drill --output $OUT $PCB
kicad-cli pcb export gerbers --output $OUT $PCB
cd target/kicad
zip $ZIP rukey_thumb_cluster_gerber_files/*
mv $ZIP ../..
cd -

kicad-cli pcb export step --subst-models --output rukey_thumb_cluster_pcb.step $PCB
