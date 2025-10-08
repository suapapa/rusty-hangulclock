union(){
    difference() {
        translate([0,2.5,0]) cube ([170, 15 , 10], center=true);
        translate([12.5, 0, 3]) cylinder(10, 1.5, 1.5, center=true);
        translate([-12.5, 0, 3]) cylinder(10, 1.5, 1.5, center=true);
        
        // wall nail hole
        translate([0,0,0]) cylinder(11, 4, 4, center=true);
        translate([0,-6,0]) cylinder(11, 6, 6, center=true);
        translate([0,0,2]) cylinder(10, 7, 7, center=true);
        
        // sideholes
        translate([170/2,5,0.6]) rotate([0,90.0]) cylinder(20, 1.5, 1.5, center=true);
        translate([-170/2,5,0.6]) rotate([0,90.0]) cylinder(20, 1.5, 1.5, center=true);
    }
    translate([53.5, 0, 2]) cylinder(10, 1.4, 1.4, center=true, $fn=20);
    translate([-53.5, 0, 2]) cylinder(10, 1.4, 1.4, center=true, $fn=20);
}
