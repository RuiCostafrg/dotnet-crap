class SwitchFixture {
    int Pick(int x) {
        switch (x) {
            case 1:
            case 2: Hits.Mark("Switch.cs", 5); return 1;
            default: Hits.Mark("Switch.cs", 6); return 0;
        }
    }
}
