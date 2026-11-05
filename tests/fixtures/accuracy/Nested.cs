class NestedFixture {
    int Outer(int x) {
        int Inner(int y) => Hits.Value("Nested.cs", 3, y);
        if (x > 0) { Hits.Mark("Nested.cs", 4); return Inner(x); }
        Hits.Mark("Nested.cs", 5); return 0;
    }
}
