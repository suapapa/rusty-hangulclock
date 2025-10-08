difference(){
    union(){
        translate([-5,-4,0]) cube([50,8,20]);
        cylinder(h=32+2, r=1.5-0.15, $fn=10);
        translate([41,0,0]) cylinder(h=32+2, r=1.5-0.15, $fn=10);
        cylinder(h=32, r=4, $fn=6);
        translate([41,0,0]) cylinder(h=32, r=4, $fn=6);
    }
    translate([-1, -5, 3]) cube([25,10,14]);
    translate([30, -5, 1]) cube([12+0.3,10,4.5]);
    translate([7.5+2.54*0,0,1]) cylinder(h=30, r=1+0.15, $fn=10);
    translate([7.5+2.54*1,0,1]) cylinder(h=30, r=1+0.15, $fn=10);
    translate([7.5+2.54*2,0,1]) cylinder(h=30, r=1+0.15, $fn=10);
    translate([7.5+2.54*3,0,1]) cylinder(h=30, r=1+0.15, $fn=10);
}