Feature: Places for planning a shopping trip
  Places are in groups (Grocery Store > LIDL, Aldi). Each place has one or more
  locations, found by typing an address. A place shows what there is to get.

  Scenario: Patrick sets up a shop, adds what to get there, and a second location
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    When Patrick adds the group "Grocery Store"
    And Patrick opens "Grocery Store"
    And Patrick adds the place "LIDL Winnenden" by picking the address "Marbacher"
    And Patrick opens "Lidl Winnenden"
    And Patrick adds the tasks "Milk" and "Bread"
    And Patrick goes back
    Then Patrick sees "Lidl Winnenden" with "2 to get"
    When Mona opens the tab "Places"
    Then Mona sees "Grocery Store" with "2 to get"
    When Patrick opens "Lidl Winnenden"
    And Patrick edits the place
    And Patrick adds the location "LIDL Backnang"
    And Patrick goes back
    Then Patrick sees the text "2 locations"
