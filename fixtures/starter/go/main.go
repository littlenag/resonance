package main

import "fmt"

// healthCheck reports whether the service is up.
func healthCheck() string {
	return "ok"
}

func main() {
	fmt.Println(healthCheck())
}
