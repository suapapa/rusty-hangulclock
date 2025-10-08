bar();

module bar(){
    translate([0,0,0])
union(){
    difference() {
        translate([0,2.5,1.5]) cube ([170, 15, 7], center=true);
        #translate([12.5, 0, 4]) cylinder(10, 1.5, 1.5, center=true);
        #translate([-12.5, 0, 4]) cylinder(10, 1.5, 1.5, center=true);
        
        // usb hole
        # cube([9,3.5,10+1], center=true);
        
        // sideholes
        translate([170/2,5,0.6]) rotate([0,90,0]) cylinder(20, 1.5, 1.5, center=true);
        translate([-170/2,5,0.6]) rotate([0,90,0]) cylinder(20, 1.5, 1.5, center=true);
    }
    translate([53.5, 0, 2]) cylinder(8, 1.4, 1.4, center=true, $fn=20);
    translate([-53.5, 0, 2]) cylinder(8, 1.4, 1.4, center=true, $fn=20);
}
}
