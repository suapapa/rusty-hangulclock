/*
difference() {
    cylinder(8, 7, 7, $fn=8);
    translate([0, 0, 0.5]) cylinder(5, 3+0.1, 3+0.1, $fn=16);
    translate([0, 0, 4.5]) cylinder(10, 6.5, 6.5, $fn=8);
}
*/


difference() {
    union() {
        // 원래 knob 형상
        cylinder(8, 7, 7, $fn=16);

        // 돌기 추가 (8개 반복)
        for (i = [0:60]) {
            rotate([0, 0, i * 6])  // 360도 / 8 = 45도 간격
            translate([7, 0, 0])  // 반지름보다 약간 바깥쪽에 위치, 높이 z=3에서 시작
            cylinder(h=8, r=1, $fn=8);  // 돌기 모양
        }
    }

    // 내부 잘라내는 구조 유지
    translate([0, 0, 0.5]) cylinder(5, 3+0.1, 3+0.1, $fn=16);
    translate([0, 0, 4.5]) cylinder(10, 6.7, 6.7, $fn=32);
}