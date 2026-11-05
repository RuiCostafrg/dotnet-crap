class NestedFixture {
    int Outer(int x) {
        int Inner(int y) => y;
        return Inner(x);
    }
}
