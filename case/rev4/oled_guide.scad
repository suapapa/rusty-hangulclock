difference(){
    cube([36+2,34+2,2.5+1], center=false);
    translate([1,1,2.5]) cube([36,34,2.5], center=false);
    translate([2,2,1]) cube([36-2,34-2,5]);
    translate([11,11,-1]) cube([16,30,5]);
}